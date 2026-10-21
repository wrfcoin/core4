use tauri::{command, State};
use serde::{Serialize, Deserialize};
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use tokio::sync::Mutex;
use log::{info, warn, error};

use crate::AppState;
use crate::mining::cpu_miner::CpuMiner;
use crate::mining::gpu_miner::GpuMiner;
use crate::blockchain::wallet::Wallet;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MiningResponse {
    pub success: bool,
    pub data: Option<serde_json::Value>,
    pub error: Option<String>,
}

// Stores the current state of miners
struct MiningState {
    cpu_miner: Option<Arc<Mutex<CpuMiner>>>,
    gpu_miner: Option<Arc<Mutex<GpuMiner>>>,
    cpu_mining_task: Option<tokio::task::JoinHandle<()>>,
    gpu_mining_task: Option<tokio::task::JoinHandle<()>>,
}

impl Default for MiningState {
    fn default() -> Self {
        MiningState {
            cpu_miner: None,
            gpu_miner: None,
            cpu_mining_task: None,
            gpu_mining_task: None,
        }
    }
}

lazy_static::lazy_static! {
    static ref MINING_STATE: Mutex<MiningState> = Mutex::new(MiningState::default());
}

#[command]
pub async fn start_cpu_mining(
    state: State<'_, AppState>,
    miner_address: String,
    threads: Option<usize>,
) -> MiningResponse {
    info!("Command called: start_cpu_mining for address {} with {} threads", 
          miner_address, threads.unwrap_or_else(|| std::thread::available_parallelism().map(|p| p.get()).unwrap_or(1)));
    
    let mut mining_state = MINING_STATE.lock().await;
    
    // Check if mining is already running
    if mining_state.cpu_miner.is_some() && mining_state.cpu_mining_task.is_some() {
        return MiningResponse {
            success: false,
            data: None,
            error: Some("CPU mining is already running".to_string()),
        };
    }
    
    // Create new CPU miner
    let mut cpu_miner = CpuMiner::new(miner_address.clone(), threads);
    
    // Start mining
    cpu_miner.start();
    
    // Store the miner
    let cpu_miner = Arc::new(Mutex::new(cpu_miner));
    mining_state.cpu_miner = Some(cpu_miner.clone());
    
    // Start mining task
    let blockchain = state.blockchain.clone();
    let mining_task = tokio::spawn(async move {
        let mut miner = cpu_miner.lock().await;
        miner.run(blockchain).await;
    });
    
    mining_state.cpu_mining_task = Some(mining_task);
    
    MiningResponse {
        success: true,
        data: Some(serde_json::json!({
            "message": "CPU mining started",
            "address": miner_address,
            "threads": threads,
        })),
        error: None,
    }
}

#[command]
pub async fn stop_cpu_mining() -> MiningResponse {
    info!("Command called: stop_cpu_mining");
    
    let mut mining_state = MINING_STATE.lock().await;
    
    if let Some(cpu_miner) = &mining_state.cpu_miner {
        // Stop the miner
        let mut miner = cpu_miner.lock().await;
        miner.stop();
        
        // Abort the mining task if it exists
        if let Some(task) = mining_state.cpu_mining_task.take() {
            task.abort();
        }
        
        // Clear the miner
        mining_state.cpu_miner = None;
        
        MiningResponse {
            success: true,
            data: Some(serde_json::json!({
                "message": "CPU mining stopped",
            })),
            error: None,
        }
    } else {
        MiningResponse {
            success: false,
            data: None,
            error: Some("No CPU mining is currently running".to_string()),
        }
    }
}

#[command]
pub async fn start_gpu_mining(
    state: State<'_, AppState>,
    miner_address: String,
    device_index: Option<usize>,
) -> MiningResponse {
    info!("Command called: start_gpu_mining for address {} with device index {:?}", 
          miner_address, device_index);
    
    let mut mining_state = MINING_STATE.lock().await;
    
    // Check if mining is already running
    if mining_state.gpu_miner.is_some() && mining_state.gpu_mining_task.is_some() {
        return MiningResponse {
            success: false,
            data: None,
            error: Some("GPU mining is already running".to_string()),
        };
    }
    
    // Create new GPU miner
    let gpu_miner = match GpuMiner::new(miner_address.clone()) {
        Ok(miner) => miner,
        Err(e) => {
            error!("Failed to create GPU miner: {}", e);
            return MiningResponse {
                success: false,
                data: None,
                error: Some(format!("Failed to create GPU miner: {}", e)),
            };
        }
    };
    
    // Get available devices
    let available_devices = gpu_miner.get_available_devices();
    
    if available_devices.is_empty() {
        return MiningResponse {
            success: false,
            data: None,
            error: Some("No compatible GPU devices found".to_string()),
        };
    }
    
    // Select the device
    let mut gpu_miner = gpu_miner;
    let device_idx = device_index.unwrap_or(0);
    
    if device_idx >= available_devices.len() {
        return MiningResponse {
            success: false,
            data: None,
            error: Some(format!("Invalid device index: {}. Available devices: {}", 
                              device_idx, available_devices.len())),
        };
    }
    
    if let Err(e) = gpu_miner.select_device(device_idx) {
        return MiningResponse {
            success: false,
            data: None,
            error: Some(format!("Failed to select GPU device: {}", e)),
        };
    }
    
    // Store the miner
    let gpu_miner = Arc::new(Mutex::new(gpu_miner));
    mining_state.gpu_miner = Some(gpu_miner.clone());
    
    // Start mining task
    let blockchain = state.blockchain.clone();
    let mining_task = tokio::spawn(async move {
        let mut miner = gpu_miner.lock().await;
        miner.run(blockchain).await;
    });
    
    mining_state.gpu_mining_task = Some(mining_task);
    
    MiningResponse {
        success: true,
        data: Some(serde_json::json!({
            "message": "GPU mining started",
            "address": miner_address,
            "device": available_devices[device_idx],
        })),
        error: None,
    }
}

#[command]
pub async fn stop_gpu_mining() -> MiningResponse {
    info!("Command called: stop_gpu_mining");
    
    let mut mining_state = MINING_STATE.lock().await;
    
    if let Some(gpu_miner) = &mining_state.gpu_miner {
        // Stop the miner
        let mut miner = gpu_miner.lock().await;
        miner.stop();
        
        // Abort the mining task if it exists
        if let Some(task) = mining_state.gpu_mining_task.take() {
            task.abort();
        }
        
        // Clear the miner
        mining_state.gpu_miner = None;
        
        MiningResponse {
            success: true,
            data: Some(serde_json::json!({
                "message": "GPU mining stopped",
            })),
            error: None,
        }
    } else {
        MiningResponse {
            success: false,
            data: None,
            error: Some("No GPU mining is currently running".to_string()),
        }
    }
}

#[command]
pub async fn get_mining_stats() -> MiningResponse {
    info!("Command called: get_mining_stats");
    
    let mining_state = MINING_STATE.lock().await;
    
    let mut stats = serde_json::json!({
        "cpu": {
            "is_mining": false,
            "cores_used": 0,
            "hashrate": 0.0,
            "blocks_mined": 0,
            "time_mining": 0,
            "rewards_earned": 0,
        },
        "gpu": {
            "is_mining": false,
            "device": null,
            "hashrate": 0.0,
            "blocks_mined": 0,
            "time_mining": 0,
            "rewards_earned": 0,
        }
    });
    
    // Update CPU stats if mining
    if let Some(cpu_miner) = &mining_state.cpu_miner {
        let miner = cpu_miner.lock().await;
        stats["cpu"] = serde_json::json!({
            "is_mining": miner.stats.is_mining,
            "cores_used": miner.stats.cores_used,
            "hashrate": miner.stats.hashrate,
            "blocks_mined": miner.stats.blocks_mined,
            "time_mining": miner.stats.time_mining,
            "last_block_time": miner.stats.last_block_time,
            "rewards_earned": miner.stats.rewards_earned,
        });
    }
    
    // Update GPU stats if mining
    if let Some(gpu_miner) = &mining_state.gpu_miner {
        let miner = gpu_miner.lock().await;
        stats["gpu"] = serde_json::json!({
            "is_mining": miner.stats.is_mining,
            "device": miner.stats.device,
            "hashrate": miner.stats.hashrate,
            "blocks_mined": miner.stats.blocks_mined,
            "time_mining": miner.stats.time_mining,
            "last_block_time": miner.stats.last_block_time,
            "rewards_earned": miner.stats.rewards_earned,
            "temperature": miner.stats.temperature,
            "power_usage": miner.stats.power_usage,
        });
    }
    
    MiningResponse {
        success: true,
        data: Some(stats),
        error: None,
    }
}

#[command]
pub async fn get_available_gpu_devices() -> MiningResponse {
    info!("Command called: get_available_gpu_devices");
    
    // Create a temporary GPU miner to get device info
    let gpu_miner = match GpuMiner::new("temporary".to_string()) {
        Ok(miner) => miner,
        Err(e) => {
            error!("Failed to create GPU miner: {}", e);
            return MiningResponse {
                success: false,
                data: None,
                error: Some(format!("Failed to create GPU miner: {}", e)),
            };
        }
    };
    
    let devices = gpu_miner.get_available_devices();
    
    MiningResponse {
        success: true,
        data: Some(serde_json::json!(devices)),
        error: None,
    }
}

#[command]
pub async fn one_click_mine(
    state: State<'_, AppState>,
    miner_address: String,
) -> MiningResponse {
    info!("Command called: one_click_mine for address {}", miner_address);
    
    let mut mining_state = MINING_STATE.lock().await;
    
    // Stop any running miners
    if let Some(cpu_miner) = &mining_state.cpu_miner {
        let mut miner = cpu_miner.lock().await;
        miner.stop();
        
        if let Some(task) = mining_state.cpu_mining_task.take() {
            task.abort();
        }
    }
    
    if let Some(gpu_miner) = &mining_state.gpu_miner {
        let mut miner = gpu_miner.lock().await;
        miner.stop();
        
        if let Some(task) = mining_state.gpu_mining_task.take() {
            task.abort();
        }
    }
    
    // Clear current miners
    mining_state.cpu_miner = None;
    mining_state.gpu_miner = None;
    
    // Create wallet if it doesn't exist
    let db = state.database.lock().await;
    if let Ok(None) = db.get_wallet_by_address(&miner_address) {
        info!("Creating new wallet for address: {}", miner_address);
        let wallet = Wallet::new();
        if let Err(e) = db.save_wallet(&wallet) {
            error!("Failed to save wallet: {}", e);
        }
    }
    
    // Determine available resources
    let cpu_threads = std::thread::available_parallelism()
        .map(|p| p.get())
        .unwrap_or(1);
    
    // Reserve 1 thread for system if multiple threads available
    let cpu_threads_to_use = if cpu_threads > 1 { cpu_threads - 1 } else { 1 };
    
    // Start CPU mining
    let mut cpu_miner = CpuMiner::new(miner_address.clone(), Some(cpu_threads_to_use));
    cpu_miner.start();
    
    let cpu_miner = Arc::new(Mutex::new(cpu_miner));
    mining_state.cpu_miner = Some(cpu_miner.clone());
    
    let blockchain_for_cpu = state.blockchain.clone();
    let cpu_mining_task = tokio::spawn(async move {
        let mut miner = cpu_miner.lock().await;
        miner.run(blockchain_for_cpu).await;
    });
    
    mining_state.cpu_mining_task = Some(cpu_mining_task);
    
    // Try to start GPU mining if available
    let gpu_mining_result = match GpuMiner::new(miner_address.clone()) {
        Ok(mut gpu_miner) => {
            let devices = gpu_miner.get_available_devices();
            
            if !devices.is_empty() {
                if let Err(e) = gpu_miner.select_device(0) {
                    warn!("Failed to select GPU device: {}", e);
                    None
                } else {
                    let gpu_miner = Arc::new(Mutex::new(gpu_miner));
                    mining_state.gpu_miner = Some(gpu_miner.clone());
                    
                    let blockchain_for_gpu = state.blockchain.clone();
                    let gpu_mining_task = tokio::spawn(async move {
                        let mut miner = gpu_miner.lock().await;
                        miner.run(blockchain_for_gpu).await;
                    });
                    
                    mining_state.gpu_mining_task = Some(gpu_mining_task);
                    
                    Some(devices[0].clone())
                }
            } else {
                None
            }
        },
        Err(e) => {
            warn!("Failed to create GPU miner: {}", e);
            None
        }
    };
    
    MiningResponse {
        success: true,
        data: Some(serde_json::json!({
            "message": "One-click mining started",
            "address": miner_address,
            "cpu": {
                "enabled": true,
                "threads": cpu_threads_to_use,
            },
            "gpu": {
                "enabled": gpu_mining_result.is_some(),
                "device": gpu_mining_result,
            }
        })),
        error: None,
    }
}

#[command]
pub async fn stop_all_mining() -> MiningResponse {
    info!("Command called: stop_all_mining");
    
    let mut mining_state = MINING_STATE.lock().await;
    
    // Stop CPU mining if running
    if let Some(cpu_miner) = &mining_state.cpu_miner {
        let mut miner = cpu_miner.lock().await;
        miner.stop();
        
        if let Some(task) = mining_state.cpu_mining_task.take() {
            task.abort();
        }
        
        mining_state.cpu_miner = None;
    }
    
    // Stop GPU mining if running
    if let Some(gpu_miner) = &mining_state.gpu_miner {
        let mut miner = gpu_miner.lock().await;
        miner.stop();
        
        if let Some(task) = mining_state.gpu_mining_task.take() {
            task.abort();
        }
        
        mining_state.gpu_miner = None;
    }
    
    MiningResponse {
        success: true,
        data: Some(serde_json::json!({
            "message": "All mining stopped",
        })),
        error: None,
    }
}
