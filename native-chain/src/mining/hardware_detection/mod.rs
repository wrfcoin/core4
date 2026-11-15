//! Hardware Detection Module
//!
//! This module provides automated hardware detection capabilities for mining,
//! allowing the system to determine optimal mining configurations based on
//! available hardware.

use anyhow::{Result, anyhow};
use log::debug;
use serde::{Deserialize, Serialize};
use std::process::Command;
use std::sync::{Arc, Mutex};

/// CPU information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpuInfo {
    /// Number of physical cores
    pub physical_cores: u32,

    /// Number of logical cores (threads)
    pub logical_cores: u32,

    /// CPU model name
    pub model_name: String,

    /// CPU architecture (x86_64, ARM, etc.)
    pub architecture: String,

    /// CPU frequency in MHz
    pub frequency_mhz: f64,

    /// CPU features (AVX, SSE, etc.)
    pub features: Vec<String>,

    /// L1 cache size in KB
    pub l1_cache_kb: Option<u32>,

    /// L2 cache size in KB
    pub l2_cache_kb: Option<u32>,

    /// L3 cache size in KB
    pub l3_cache_kb: Option<u32>,
}

impl Default for CpuInfo {
    fn default() -> Self {
        Self {
            physical_cores: 1,
            logical_cores: 1,
            model_name: "Unknown".to_string(),
            architecture: "Unknown".to_string(),
            frequency_mhz: 0.0,
            features: Vec::new(),
            l1_cache_kb: None,
            l2_cache_kb: None,
            l3_cache_kb: None,
        }
    }
}

/// GPU information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuInfo {
    /// GPU model name
    pub model_name: String,

    /// GPU vendor (NVIDIA, AMD, Intel, etc.)
    pub vendor: String,

    /// GPU memory in MB
    pub memory_mb: u32,

    /// GPU compute capability (for NVIDIA)
    pub compute_capability: Option<String>,

    /// CUDA cores (for NVIDIA) or Stream Processors (for AMD)
    pub core_count: Option<u32>,

    /// GPU clock speed in MHz
    pub clock_mhz: Option<f64>,

    /// PCI bus ID
    pub pci_bus_id: Option<String>,

    /// Whether this GPU supports mining
    pub supports_mining: bool,
}

impl Default for GpuInfo {
    fn default() -> Self {
        Self {
            model_name: "Unknown".to_string(),
            vendor: "Unknown".to_string(),
            memory_mb: 0,
            compute_capability: None,
            core_count: None,
            clock_mhz: None,
            pci_bus_id: None,
            supports_mining: false,
        }
    }
}

/// Memory information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryInfo {
    /// Total physical memory in MB
    pub total_mb: u64,

    /// Free physical memory in MB
    pub free_mb: u64,

    /// Memory type (DDR4, DDR5, etc.)
    pub memory_type: Option<String>,

    /// Memory frequency in MHz
    pub frequency_mhz: Option<u32>,

    /// Number of memory channels
    pub channels: Option<u32>,
}

impl Default for MemoryInfo {
    fn default() -> Self {
        Self {
            total_mb: 0,
            free_mb: 0,
            memory_type: None,
            frequency_mhz: None,
            channels: None,
        }
    }
}

/// ML Accelerator information (TPUs, Neural Engine, etc.)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcceleratorInfo {
    /// Accelerator model name
    pub model_name: String,

    /// Accelerator vendor
    pub vendor: String,

    /// Accelerator type (TPU, NPU, etc.)
    pub accelerator_type: String,

    /// Compute capacity in TOPS or similar metric
    pub compute_capacity: Option<f64>,

    /// Available memory in MB
    pub memory_mb: Option<u32>,

    /// Whether this accelerator supports mining
    pub supports_mining: bool,
}

impl Default for AcceleratorInfo {
    fn default() -> Self {
        Self {
            model_name: "Unknown".to_string(),
            vendor: "Unknown".to_string(),
            accelerator_type: "Unknown".to_string(),
            compute_capacity: None,
            memory_mb: None,
            supports_mining: false,
        }
    }
}

/// Architecture type for mining
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Architecture {
    /// x86_64 architecture
    X86_64,

    /// ARM 64-bit architecture
    ARM64,

    /// ARM 32-bit architecture
    ARM32,

    /// RISC-V architecture
    RISCV,

    /// WebAssembly architecture
    WASM,

    /// Other architecture
    Other,
}

impl Default for Architecture {
    fn default() -> Self {
        Architecture::X86_64
    }
}

impl std::fmt::Display for Architecture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Architecture::X86_64 => write!(f, "x86_64"),
            Architecture::ARM64 => write!(f, "arm64"),
            Architecture::ARM32 => write!(f, "arm32"),
            Architecture::RISCV => write!(f, "riscv"),
            Architecture::WASM => write!(f, "wasm"),
            Architecture::Other => write!(f, "other"),
        }
    }
}

/// Hardware capabilities for mining
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct HardwareCapabilities {
    /// CPU information
    pub cpu: CpuInfo,

    /// GPU information (if available)
    pub gpus: Vec<GpuInfo>,

    /// Memory information
    pub memory: MemoryInfo,

    /// ML Accelerator information (if available)
    pub accelerators: Vec<AcceleratorInfo>,

    /// System architecture
    pub architecture: Architecture,

    /// Operating system
    pub os: String,

    /// Kernel version
    pub kernel_version: Option<String>,
}

/// Mining hardware configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareConfig {
    /// Number of CPU threads to use for mining
    pub cpu_threads: u32,

    /// Selected GPUs for mining (indices into the HardwareCapabilities.gpus vector)
    pub selected_gpus: Vec<usize>,

    /// Selected accelerators for mining (indices into the HardwareCapabilities.accelerators vector)
    pub selected_accelerators: Vec<usize>,

    /// Memory limit for mining in MB
    pub memory_limit_mb: u64,

    /// Architecture-specific optimizations to enable
    pub optimizations: Vec<String>,

    /// Mining intensity (0.0 to 1.0)
    pub intensity: f64,
}

impl Default for HardwareConfig {
    fn default() -> Self {
        Self {
            cpu_threads: 1,
            selected_gpus: Vec::new(),
            selected_accelerators: Vec::new(),
            memory_limit_mb: 1024,
            optimizations: Vec::new(),
            intensity: 0.5,
        }
    }
}

/// Hardware detector for mining
pub struct HardwareDetector {
    /// Cached hardware capabilities
    capabilities: Arc<Mutex<Option<HardwareCapabilities>>>,
}

impl Default for HardwareDetector {
    fn default() -> Self {
        Self {
            capabilities: Arc::new(Mutex::new(None)),
        }
    }
}

impl HardwareDetector {
    /// Create a new hardware detector
    pub fn new() -> Self {
        Self::default()
    }

    /// Detect hardware capabilities
    pub fn detect_capabilities(&self) -> Result<HardwareCapabilities> {
        // Check if we already have cached capabilities
        if let Some(capabilities) = self
            .capabilities
            .lock()
            .map_err(|_| anyhow!("hardware capabilities mutex poisoned"))?
            .as_ref()
        {
            return Ok(capabilities.clone());
        }

        // Detect CPU information
        let cpu_info = self.detect_cpu_info()?;

        // Detect GPU information
        let gpus = self.detect_gpu_info()?;

        // Detect memory information
        let memory_info = self.detect_memory_info()?;

        // Detect ML accelerator information
        let accelerators = self.detect_accelerator_info()?;

        // Detect system architecture
        let arch = self.detect_architecture()?;

        // Detect operating system
        let (os, kernel_version) = self.detect_os_info()?;

        // Create hardware capabilities
        let capabilities = HardwareCapabilities {
            cpu: cpu_info,
            gpus,
            memory: memory_info,
            accelerators,
            architecture: arch,
            os,
            kernel_version,
        };

        // Cache the capabilities
        *self
            .capabilities
            .lock()
            .map_err(|_| anyhow!("hardware capabilities mutex poisoned"))? =
            Some(capabilities.clone());

        Ok(capabilities)
    }

    /// Detect CPU information
    fn detect_cpu_info(&self) -> Result<CpuInfo> {
        let mut info = CpuInfo::default();

        #[cfg(target_os = "linux")]
        {
            // On Linux, read from /proc/cpuinfo
            if let Ok(cpuinfo) = std::fs::read_to_string("/proc/cpuinfo") {
                let mut cores_seen = std::collections::HashSet::new();
                let mut physical_cores = 0;

                for line in cpuinfo.lines() {
                    if line.starts_with("model name") {
                        if info.model_name == "Unknown" {
                            if let Some(model) = line.split(':').nth(1) {
                                info.model_name = model.trim().to_string();
                            }
                        }
                    } else if line.starts_with("physical id") {
                        if let Some(id_str) = line.split(':').nth(1) {
                            if let Ok(id) = id_str.trim().parse::<u32>() {
                                cores_seen.insert(id);
                            }
                        }
                    } else if line.starts_with("cpu cores") {
                        if let Some(cores_str) = line.split(':').nth(1) {
                            if let Ok(cores) = cores_str.trim().parse::<u32>() {
                                physical_cores = cores;
                            }
                        }
                    } else if line.starts_with("flags") || line.starts_with("Features") {
                        if let Some(flags) = line.split(':').nth(1) {
                            info.features = flags
                                .trim()
                                .split_whitespace()
                                .map(|s| s.to_string())
                                .collect();
                        }
                    }
                }

                info.physical_cores = physical_cores * cores_seen.len() as u32;
            }

            // Get CPU frequency
            if let Ok(freq) =
                std::fs::read_to_string("/sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_max_freq")
            {
                if let Ok(freq) = freq.trim().parse::<u64>() {
                    // Convert from kHz to MHz
                    info.frequency_mhz = freq as f64 / 1000.0;
                }
            }

            // Get logical cores count using nproc
            if let Ok(output) = Command::new("nproc").output() {
                if let Ok(nproc) = String::from_utf8(output.stdout) {
                    if let Ok(logical_cores) = nproc.trim().parse::<u32>() {
                        info.logical_cores = logical_cores;
                    }
                }
            }
        }

        #[cfg(target_os = "macos")]
        {
            // Get CPU info using sysctl
            if let Ok(output) = Command::new("sysctl")
                .args(&["-n", "machdep.cpu.brand_string"])
                .output()
            {
                if let Ok(model) = String::from_utf8(output.stdout) {
                    info.model_name = model.trim().to_string();
                }
            }

            // Get physical core count
            if let Ok(output) = Command::new("sysctl")
                .args(&["-n", "hw.physicalcpu"])
                .output()
            {
                if let Ok(cores) = String::from_utf8(output.stdout) {
                    if let Ok(physical_cores) = cores.trim().parse::<u32>() {
                        info.physical_cores = physical_cores;
                    }
                }
            }

            // Get logical core count
            if let Ok(output) = Command::new("sysctl")
                .args(&["-n", "hw.logicalcpu"])
                .output()
            {
                if let Ok(cores) = String::from_utf8(output.stdout) {
                    if let Ok(logical_cores) = cores.trim().parse::<u32>() {
                        info.logical_cores = logical_cores;
                    }
                }
            }

            // Get CPU frequency
            if let Ok(output) = Command::new("sysctl")
                .args(&["-n", "hw.cpufrequency"])
                .output()
            {
                if let Ok(freq) = String::from_utf8(output.stdout) {
                    if let Ok(frequency) = freq.trim().parse::<u64>() {
                        // Convert from Hz to MHz
                        info.frequency_mhz = frequency as f64 / 1_000_000.0;
                    }
                }
            }

            // Get CPU features
            if let Ok(output) = Command::new("sysctl")
                .args(&["-n", "machdep.cpu.features"])
                .output()
            {
                if let Ok(features) = String::from_utf8(output.stdout) {
                    info.features = features
                        .trim()
                        .split_whitespace()
                        .map(|s| s.to_lowercase())
                        .collect();
                }
            }
        }

        #[cfg(target_os = "windows")]
        {
            // On Windows, use PowerShell to get CPU info
            let args = [
                "-Command",
                "Get-WmiObject -Class Win32_Processor | Select-Object -Property Name,NumberOfCores,NumberOfLogicalProcessors,MaxClockSpeed | ConvertTo-Json",
            ];

            if let Ok(output) = Command::new("powershell").args(&args).output() {
                if let Ok(json_str) = String::from_utf8(output.stdout) {
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&json_str) {
                        if let Some(name) = json.get("Name").and_then(|v| v.as_str()) {
                            info.model_name = name.to_string();
                        }

                        if let Some(cores) = json.get("NumberOfCores").and_then(|v| v.as_u64()) {
                            info.physical_cores = cores as u32;
                        }

                        if let Some(logical) = json
                            .get("NumberOfLogicalProcessors")
                            .and_then(|v| v.as_u64())
                        {
                            info.logical_cores = logical as u32;
                        }

                        if let Some(speed) = json.get("MaxClockSpeed").and_then(|v| v.as_u64()) {
                            info.frequency_mhz = speed as f64;
                        }
                    }
                }
            }
        }

        // Set architecture based on compile-time info
        #[cfg(target_arch = "x86_64")]
        {
            info.architecture = "x86_64".to_string();
        }

        #[cfg(target_arch = "aarch64")]
        {
            info.architecture = "arm64".to_string();
        }

        #[cfg(target_arch = "arm")]
        {
            info.architecture = "arm32".to_string();
        }

        #[cfg(target_arch = "riscv64")]
        {
            info.architecture = "riscv64".to_string();
        }

        Ok(info)
    }

    /// Detect GPU information
    fn detect_gpu_info(&self) -> Result<Vec<GpuInfo>> {
        let mut gpus = Vec::new();

        // Try to detect NVIDIA GPUs using nvidia-smi
        if let Ok(output) = Command::new("nvidia-smi")
            .args(&[
                "--query-gpu=name,memory.total,uuid,pci.bus_id",
                "--format=csv,noheader",
            ])
            .output()
        {
            if output.status.success() {
                if let Ok(output_str) = String::from_utf8(output.stdout) {
                    for line in output_str.lines() {
                        let parts: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
                        if parts.len() >= 3 {
                            let model_name = parts[0].to_string();
                            let memory_str = parts[1].to_string();
                            let memory_mb = if memory_str.ends_with(" MiB") {
                                memory_str[..memory_str.len() - 4]
                                    .parse::<u32>()
                                    .unwrap_or(0)
                            } else {
                                0
                            };

                            let pci_bus_id = if parts.len() > 3 {
                                Some(parts[3].to_string())
                            } else {
                                None
                            };

                            gpus.push(GpuInfo {
                                model_name,
                                vendor: "NVIDIA".to_string(),
                                memory_mb,
                                compute_capability: None, // Can be obtained with more complex nvidia-smi query
                                core_count: None, // Can be obtained with more complex nvidia-smi query
                                clock_mhz: None, // Can be obtained with more complex nvidia-smi query
                                pci_bus_id,
                                supports_mining: true,
                            });
                        }
                    }
                }
            }
        }

        // Try to detect AMD GPUs using rocm-smi
        if let Ok(output) = Command::new("rocm-smi")
            .args(&["--showproductname"])
            .output()
        {
            if output.status.success() {
                if let Ok(output_str) = String::from_utf8(output.stdout) {
                    for line in output_str.lines() {
                        if line.contains("GPU") && line.contains("Product name") {
                            let parts: Vec<&str> = line.split(':').collect();
                            if parts.len() >= 2 {
                                let model_name = parts[1].trim().to_string();

                                gpus.push(GpuInfo {
                                    model_name,
                                    vendor: "AMD".to_string(),
                                    memory_mb: 0, // Need additional command to get memory
                                    compute_capability: None,
                                    core_count: None,
                                    clock_mhz: None,
                                    pci_bus_id: None,
                                    supports_mining: true,
                                });
                            }
                        }
                    }
                }
            }
        }

        // If we couldn't detect any GPUs, at least try to get basic info from lspci on Linux
        if gpus.is_empty() && cfg!(target_os = "linux") {
            if let Ok(output) = Command::new("lspci").args(&["-v"]).output() {
                if let Ok(output_str) = String::from_utf8(output.stdout) {
                    let mut current_gpu = None;
                    let mut current_vendor = None;

                    for line in output_str.lines() {
                        if line.contains("VGA") || line.contains("3D") || line.contains("Display") {
                            if let Some(model_start) = line.find(':') {
                                let model_name = line[model_start + 1..].trim().to_string();
                                current_gpu = Some(model_name);

                                if line.contains("NVIDIA") {
                                    current_vendor = Some("NVIDIA".to_string());
                                } else if line.contains("AMD") || line.contains("ATI") {
                                    current_vendor = Some("AMD".to_string());
                                } else if line.contains("Intel") {
                                    current_vendor = Some("Intel".to_string());
                                } else {
                                    current_vendor = Some("Unknown".to_string());
                                }
                            }
                        } else if line.contains("Memory") && current_gpu.is_some() {
                            // If we found memory info and have a current GPU, push it
                            let model_name = current_gpu.take().unwrap();
                            let vendor = current_vendor
                                .take()
                                .unwrap_or_else(|| "Unknown".to_string());

                            // Try to parse memory info
                            let memory_mb = if let Some(mem_start) = line.find("Memory") {
                                if let Some(mem_end) = line[mem_start..].find("MB") {
                                    let mem_str = &line[mem_start..mem_start + mem_end];
                                    let digits: String =
                                        mem_str.chars().filter(|c| c.is_digit(10)).collect();
                                    digits.parse::<u32>().unwrap_or(0)
                                } else {
                                    0
                                }
                            } else {
                                0
                            };

                            gpus.push(GpuInfo {
                                model_name,
                                vendor,
                                memory_mb,
                                compute_capability: None,
                                core_count: None,
                                clock_mhz: None,
                                pci_bus_id: None,
                                supports_mining: true,
                            });
                        }
                    }

                    // If we still have a GPU in progress, push it
                    if let Some(model_name) = current_gpu {
                        let vendor = current_vendor.unwrap_or_else(|| "Unknown".to_string());

                        gpus.push(GpuInfo {
                            model_name,
                            vendor,
                            memory_mb: 0,
                            compute_capability: None,
                            core_count: None,
                            clock_mhz: None,
                            pci_bus_id: None,
                            supports_mining: true,
                        });
                    }
                }
            }
        }

        Ok(gpus)
    }

    /// Detect memory information
    fn detect_memory_info(&self) -> Result<MemoryInfo> {
        let mut info = MemoryInfo::default();

        #[cfg(target_os = "linux")]
        {
            // On Linux, read from /proc/meminfo
            if let Ok(meminfo) = std::fs::read_to_string("/proc/meminfo") {
                for line in meminfo.lines() {
                    if line.starts_with("MemTotal:") {
                        if let Some(mem_str) = line.split(':').nth(1) {
                            let mem_kb: u64 = mem_str
                                .trim()
                                .split_whitespace()
                                .next()
                                .unwrap_or("0")
                                .parse()
                                .unwrap_or(0);
                            info.total_mb = mem_kb / 1024;
                        }
                    } else if line.starts_with("MemFree:") {
                        if let Some(mem_str) = line.split(':').nth(1) {
                            let mem_kb: u64 = mem_str
                                .trim()
                                .split_whitespace()
                                .next()
                                .unwrap_or("0")
                                .parse()
                                .unwrap_or(0);
                            info.free_mb = mem_kb / 1024;
                        }
                    }
                }
            }
        }

        #[cfg(target_os = "macos")]
        {
            // Get total memory
            if let Ok(output) = Command::new("sysctl").args(&["-n", "hw.memsize"]).output() {
                if let Ok(mem) = String::from_utf8(output.stdout) {
                    if let Ok(total_bytes) = mem.trim().parse::<u64>() {
                        info.total_mb = total_bytes / (1024 * 1024);
                    }
                }
            }

            // Get free memory using vm_stat
            if let Ok(output) = Command::new("vm_stat").output() {
                if let Ok(vmstat) = String::from_utf8(output.stdout) {
                    let page_size = if let Ok(output) =
                        Command::new("sysctl").args(&["-n", "hw.pagesize"]).output()
                    {
                        if let Ok(size) = String::from_utf8(output.stdout) {
                            size.trim().parse::<u64>().unwrap_or(4096)
                        } else {
                            4096
                        }
                    } else {
                        4096
                    };

                    let mut free_pages = 0;

                    for line in vmstat.lines() {
                        if line.starts_with("Pages free:") {
                            if let Some(pages_str) = line.rsplitn(2, ':').next() {
                                let pages: u64 =
                                    pages_str.trim().trim_end_matches('.').parse().unwrap_or(0);
                                free_pages += pages;
                            }
                        } else if line.starts_with("Pages inactive:") {
                            if let Some(pages_str) = line.rsplitn(2, ':').next() {
                                let pages: u64 =
                                    pages_str.trim().trim_end_matches('.').parse().unwrap_or(0);
                                free_pages += pages;
                            }
                        }
                    }

                    info.free_mb = (free_pages * page_size) / (1024 * 1024);
                }
            }
        }

        #[cfg(target_os = "windows")]
        {
            // On Windows, use PowerShell to get memory info
            let args = [
                "-Command",
                "Get-WmiObject -Class Win32_OperatingSystem | Select-Object -Property TotalVisibleMemorySize,FreePhysicalMemory | ConvertTo-Json",
            ];

            if let Ok(output) = Command::new("powershell").args(&args).output() {
                if let Ok(json_str) = String::from_utf8(output.stdout) {
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&json_str) {
                        if let Some(total) =
                            json.get("TotalVisibleMemorySize").and_then(|v| v.as_u64())
                        {
                            // Convert from KB to MB
                            info.total_mb = total / 1024;
                        }

                        if let Some(free) = json.get("FreePhysicalMemory").and_then(|v| v.as_u64())
                        {
                            // Convert from KB to MB
                            info.free_mb = free / 1024;
                        }
                    }
                }
            }
        }

        Ok(info)
    }

    /// Detect ML accelerator information
    fn detect_accelerator_info(&self) -> Result<Vec<AcceleratorInfo>> {
        let mut accelerators = Vec::new();

        // Check for TPUs (Google Cloud)
        if let Ok(output) = Command::new("lspci").output() {
            if let Ok(lspci_output) = String::from_utf8(output.stdout) {
                if lspci_output.contains("Google, Inc.") && lspci_output.contains("TPU") {
                    accelerators.push(AcceleratorInfo {
                        model_name: "Cloud TPU".to_string(),
                        vendor: "Google".to_string(),
                        accelerator_type: "TPU".to_string(),
                        compute_capacity: None,
                        memory_mb: None,
                        supports_mining: true,
                    });
                }
            }
        }

        // Check for Apple Neural Engine
        #[cfg(target_os = "macos")]
        {
            if let Ok(output) = Command::new("sysctl")
                .args(&["-n", "machdep.cpu.brand_string"])
                .output()
            {
                if let Ok(brand) = String::from_utf8(output.stdout) {
                    if brand.contains("Apple") {
                        // Check if this is an M-series processor
                        if let Ok(output) = Command::new("sysctl")
                            .args(&["-n", "hw.optional.arm64"])
                            .output()
                        {
                            if let Ok(is_arm) = String::from_utf8(output.stdout) {
                                if is_arm.trim() == "1" {
                                    // This is likely an M-series Mac with Neural Engine
                                    accelerators.push(AcceleratorInfo {
                                        model_name: "Apple Neural Engine".to_string(),
                                        vendor: "Apple".to_string(),
                                        accelerator_type: "NPU".to_string(),
                                        compute_capacity: Some(11.0), // Rough estimate for M1
                                        memory_mb: None,              // Shared with system memory
                                        supports_mining: true,
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }

        // This is a placeholder - actual detection depends on available hardware and drivers
        // For a production system, we'd use more specialized APIs to detect various ML accelerators

        Ok(accelerators)
    }

    /// Detect system architecture
    fn detect_architecture(&self) -> Result<Architecture> {
        #[cfg(target_arch = "x86_64")]
        return Ok(Architecture::X86_64);

        #[cfg(target_arch = "aarch64")]
        return Ok(Architecture::ARM64);

        #[cfg(target_arch = "arm")]
        return Ok(Architecture::ARM32);

        #[cfg(target_arch = "riscv64")]
        return Ok(Architecture::RISCV);

        #[cfg(target_arch = "wasm32")]
        return Ok(Architecture::WASM);

        #[allow(unreachable_code)]
        Ok(Architecture::Other)
    }

    /// Detect operating system info
    fn detect_os_info(&self) -> Result<(String, Option<String>)> {
        let os_name;
        let mut kernel_version = None;

        #[cfg(target_os = "linux")]
        {
            os_name = String::from("Linux");
            if let Ok(output) = Command::new("uname").args(&["-r"]).output() {
                if let Ok(version) = String::from_utf8(output.stdout) {
                    kernel_version = Some(version.trim().to_string());
                }
            }
        }

        // `os_name` is assigned exactly once per target: each branch works out its
        // value first, because a second assignment does not compile (E0384).
        #[cfg(target_os = "macos")]
        {
            let product_version = Command::new("sw_vers")
                .args(&["-productVersion"])
                .output()
                .ok()
                .and_then(|output| String::from_utf8(output.stdout).ok());
            os_name = match product_version {
                Some(version) => format!("macOS {}", version.trim()),
                None => String::from("macOS"),
            };

            if let Ok(output) = Command::new("uname").args(&["-r"]).output() {
                if let Ok(version) = String::from_utf8(output.stdout) {
                    kernel_version = Some(version.trim().to_string());
                }
            }
        }

        #[cfg(target_os = "windows")]
        {
            let mut caption_name = None;
            let args = [
                "-Command",
                "Get-WmiObject -Class Win32_OperatingSystem | Select-Object -Property Caption,Version | ConvertTo-Json",
            ];

            if let Ok(output) = Command::new("powershell").args(&args).output() {
                if let Ok(json_str) = String::from_utf8(output.stdout) {
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&json_str) {
                        if let Some(caption) = json.get("Caption").and_then(|v| v.as_str()) {
                            caption_name = Some(caption.to_string());
                        }

                        if let Some(version) = json.get("Version").and_then(|v| v.as_str()) {
                            kernel_version = Some(version.to_string());
                        }
                    }
                }
            }
            os_name = caption_name.unwrap_or_else(|| String::from("Windows"));
        }

        Ok((os_name, kernel_version))
    }

    /// Create optimal hardware configuration for mining based on detected capabilities
    pub fn create_optimal_config(
        &self,
        capabilities: &HardwareCapabilities,
    ) -> Result<HardwareConfig> {
        let mut config = HardwareConfig::default();

        // Configure CPU threads
        // Use about 75% of logical cores for mining, leaving some for OS
        config.cpu_threads =
            std::cmp::max(1, (capabilities.cpu.logical_cores as f64 * 0.75) as u32);

        // Set memory limit to 75% of available memory
        config.memory_limit_mb = capabilities.memory.total_mb * 3 / 4;

        // Select GPUs that support mining
        for (i, gpu) in capabilities.gpus.iter().enumerate() {
            if gpu.supports_mining {
                config.selected_gpus.push(i);
            }
        }

        // Select accelerators that support mining
        for (i, acc) in capabilities.accelerators.iter().enumerate() {
            if acc.supports_mining {
                config.selected_accelerators.push(i);
            }
        }

        // Set default mining intensity to 0.6 (moderate)
        config.intensity = 0.6;

        // Add architecture-specific optimizations
        match capabilities.architecture {
            Architecture::X86_64 => {
                if capabilities.cpu.features.iter().any(|f| f.contains("avx2")) {
                    config.optimizations.push("avx2".to_string());
                } else if capabilities.cpu.features.iter().any(|f| f.contains("avx")) {
                    config.optimizations.push("avx".to_string());
                } else if capabilities
                    .cpu
                    .features
                    .iter()
                    .any(|f| f.contains("sse4.1"))
                {
                    config.optimizations.push("sse4.1".to_string());
                }
            },
            Architecture::ARM64 => {
                config.optimizations.push("neon".to_string());
            },
            _ => {},
        }

        Ok(config)
    }
}

/// Hardware manager for mining
pub struct HardwareManager {
    /// Hardware detector
    detector: HardwareDetector,

    /// Current hardware configuration
    current_config: Option<HardwareConfig>,

    /// Current hardware capabilities
    capabilities: Option<HardwareCapabilities>,
}

impl Default for HardwareManager {
    fn default() -> Self {
        Self {
            detector: HardwareDetector::new(),
            current_config: None,
            capabilities: None,
        }
    }
}

impl HardwareManager {
    /// Create a new hardware manager
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if hardware detection has been initialized
    pub fn is_initialized(&self) -> bool {
        self.capabilities.is_some()
    }

    /// Initialize hardware detection
    pub fn initialize(&mut self) -> Result<()> {
        if !self.is_initialized() {
            debug!("Initializing hardware detection");
            let caps = self.detector.detect_capabilities()?;
            self.capabilities = Some(caps);
        }
        Ok(())
    }

    /// Get hardware capabilities
    pub fn get_capabilities(&mut self) -> Result<HardwareCapabilities> {
        match &self.capabilities {
            Some(caps) => Ok(caps.clone()),
            None => {
                let caps = self.detector.detect_capabilities()?;
                self.capabilities = Some(caps.clone());
                Ok(caps)
            },
        }
    }

    /// Get optimal hardware configuration
    pub fn get_optimal_config(&mut self) -> Result<HardwareConfig> {
        let caps = self.get_capabilities()?;
        let config = self.detector.create_optimal_config(&caps)?;
        self.current_config = Some(config.clone());
        Ok(config)
    }

    /// Set current hardware configuration
    pub fn set_config(&mut self, config: HardwareConfig) -> Result<()> {
        self.current_config = Some(config);
        Ok(())
    }

    /// Get current hardware configuration
    pub fn get_current_config(&self) -> Result<HardwareConfig> {
        match &self.current_config {
            Some(config) => Ok(config.clone()),
            None => Err(anyhow!("No hardware configuration has been set")),
        }
    }

    /// Create a hardware configuration for one-click mining
    pub fn create_one_click_config(&mut self) -> Result<HardwareConfig> {
        self.get_optimal_config()
    }
}

/// Type alias for use in parent module re-exports.
/// `HardwareCapabilities` is the canonical name; `HardwareProfile` is the public alias.
pub type HardwareProfile = HardwareCapabilities;
