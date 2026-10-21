#![allow(dead_code)]

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering};
use std::thread;
use std::time::{Duration, Instant};

/// Tracks resource usage and limits for the node
pub struct ResourceManager {
    // CPU usage limit (percentage 0-100)
    cpu_limit: AtomicU8,
    // GPU usage limit (percentage 0-100)
    gpu_limit: AtomicU8,
    // Memory usage limit in MB
    memory_limit: AtomicU32,
    // Whether resource monitoring is active
    monitoring_active: AtomicBool,
    // Whether GPU is available on this system
    gpu_available: bool,
}

/// Represents the current resource usage
#[derive(Debug, Clone, Copy)]
pub struct ResourceUsage {
    pub cpu_usage: u8,
    pub gpu_usage: u8,
    pub memory_usage: u16,
    pub timestamp: u64,
}

/// Represents an error in resource management
#[derive(Debug)]
pub enum ResourceError {
    ResourceLimitExceeded(String),
    ResourceUnavailable(String),
    MonitoringError(String),
}

use std::fmt;
impl fmt::Display for ResourceError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ResourceError::ResourceLimitExceeded(msg) => {
                write!(f, "Resource limit exceeded: {msg}")
            },
            ResourceError::ResourceUnavailable(msg) => write!(f, "Resource unavailable: {msg}"),
            ResourceError::MonitoringError(msg) => write!(f, "Monitoring error: {msg}"),
        }
    }
}

/// Computation priority level
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComputationPriority {
    Low,
    Medium,
    High,
    Critical,
}

impl ResourceManager {
    /// Create a new resource manager with default limits
    pub fn new() -> Self {
        // Default to 50% CPU, 30% GPU (if available), 1GB memory
        ResourceManager {
            cpu_limit: AtomicU8::new(50),
            gpu_limit: AtomicU8::new(30),
            memory_limit: AtomicU32::new(1024),
            monitoring_active: AtomicBool::new(false),
            // In a real implementation, this would detect if GPU is available
            gpu_available: false,
        }
    }

    /// Set CPU usage limit (0-100%)
    pub fn set_cpu_limit(&self, limit: u8) {
        let limit = limit.min(100);
        self.cpu_limit.store(limit, Ordering::SeqCst);
    }

    /// Get current CPU usage limit
    pub fn get_cpu_limit(&self) -> u8 {
        self.cpu_limit.load(Ordering::SeqCst)
    }

    /// Set GPU usage limit (0-100%)
    pub fn set_gpu_limit(&self, limit: u8) -> Result<(), ResourceError> {
        if !self.gpu_available {
            return Err(ResourceError::ResourceUnavailable(
                "GPU not available on this system".to_string(),
            ));
        }

        let limit = limit.min(100);
        self.gpu_limit.store(limit, Ordering::SeqCst);
        Ok(())
    }

    /// Get current GPU usage limit
    pub fn get_gpu_limit(&self) -> u8 {
        self.gpu_limit.load(Ordering::SeqCst)
    }

    /// Set memory usage limit in MB
    pub fn set_memory_limit(&self, limit_mb: u16) {
        self.memory_limit
            .store(u32::from(limit_mb), Ordering::SeqCst);
    }

    /// Get current memory usage limit in MB
    pub fn get_memory_limit(&self) -> u16 {
        self.memory_limit.load(Ordering::SeqCst) as u16
    }

    /// Start resource monitoring in a background thread
    pub fn start_monitoring(&self) -> Arc<AtomicBool> {
        self.monitoring_active.store(true, Ordering::SeqCst);
        let stop_signal = Arc::new(AtomicBool::new(false));
        let stop_signal_clone = stop_signal.clone();

        // In a real implementation, we would spawn a thread to monitor resource usage
        // For this demo, we'll just simulate it

        thread::spawn(move || {
            while !stop_signal_clone.load(Ordering::SeqCst) {
                // Sleep for a bit to not hog resources
                thread::sleep(Duration::from_secs(1));

                // In a real implementation, update actual usage metrics here
            }
        });

        stop_signal
    }

    /// Stop resource monitoring
    pub fn stop_monitoring(&self, stop_signal: Arc<AtomicBool>) {
        stop_signal.store(true, Ordering::SeqCst);
        self.monitoring_active.store(false, Ordering::SeqCst);
    }

    /// Check if monitoring is active
    pub fn is_monitoring_active(&self) -> bool {
        self.monitoring_active.load(Ordering::SeqCst)
    }

    /// Get current resource usage
    pub fn get_current_usage(&self) -> ResourceUsage {
        // In a real implementation, we would get actual resource usage
        // For this demo, we'll return simulated values
        ResourceUsage {
            cpu_usage: self.cpu_limit.load(Ordering::SeqCst) / 2, // Simulate using half of allowed CPU
            gpu_usage: if self.gpu_available {
                self.gpu_limit.load(Ordering::SeqCst) / 3
            } else {
                0
            },
            memory_usage: (self.memory_limit.load(Ordering::SeqCst) / 4) as u16, // Simulate using quarter of allowed memory
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        }
    }

    /// Check if a computation can be run based on current resource usage and priority
    pub fn can_run_computation(&self, priority: ComputationPriority) -> bool {
        let usage = self.get_current_usage();

        // Logic based on priority
        match priority {
            ComputationPriority::Low => {
                // Run low priority tasks only if we're using less than 50% of limits
                usage.cpu_usage < self.cpu_limit.load(Ordering::SeqCst) / 2
                    && (!self.gpu_available
                        || usage.gpu_usage < self.gpu_limit.load(Ordering::SeqCst) / 2)
                    && u32::from(usage.memory_usage) < self.memory_limit.load(Ordering::SeqCst) / 2
            },
            ComputationPriority::Medium => {
                // Run medium priority tasks if we're using less than 75% of limits
                usage.cpu_usage < self.cpu_limit.load(Ordering::SeqCst) * 3 / 4
                    && (!self.gpu_available
                        || usage.gpu_usage < self.gpu_limit.load(Ordering::SeqCst) * 3 / 4)
                    && u32::from(usage.memory_usage)
                        < self.memory_limit.load(Ordering::SeqCst) * 3 / 4
            },
            ComputationPriority::High => {
                // Run high priority tasks if we're below limits
                usage.cpu_usage < self.cpu_limit.load(Ordering::SeqCst)
                    && (!self.gpu_available
                        || usage.gpu_usage < self.gpu_limit.load(Ordering::SeqCst))
                    && u32::from(usage.memory_usage) < self.memory_limit.load(Ordering::SeqCst)
            },
            ComputationPriority::Critical => {
                // Run critical tasks regardless of limits (user will be warned)
                true
            },
        }
    }

    /// Run a computation, respecting resource limits
    pub fn run_computation<F, T>(
        &self,
        computation: F,
        priority: ComputationPriority,
    ) -> Result<T, ResourceError>
    where
        F: FnOnce() -> T,
    {
        if !self.can_run_computation(priority) {
            return Err(ResourceError::ResourceLimitExceeded(format!(
                "Cannot run computation with priority {:?} due to resource limits",
                priority
            )));
        }

        // If this is a GPU computation, check if GPU is available
        if priority == ComputationPriority::High && !self.gpu_available {
            return Err(ResourceError::ResourceUnavailable(
                "GPU required but not available".to_string(),
            ));
        }

        // If we get here, we can run the computation
        let start_time = Instant::now();
        let result = computation();
        let elapsed = start_time.elapsed();

        // Log or emit metrics about this computation
        println!(
            "Computation completed in {:?} with priority {:?}",
            elapsed, priority
        );

        Ok(result)
    }
}

impl std::error::Error for ResourceError {}

/// Manage a specific computational task type
pub struct ComputationalTask {
    pub name: String,
    pub description: String,
    pub priority: ComputationPriority,
    pub estimated_cpu_usage: u8,
    pub estimated_gpu_usage: u8,
    pub estimated_memory_usage: u16,
    pub requires_gpu: bool,
}

impl ComputationalTask {
    /// Create a new computational task
    pub fn new(
        name: String,
        description: String,
        priority: ComputationPriority,
        estimated_cpu_usage: u8,
        estimated_gpu_usage: u8,
        estimated_memory_usage: u16,
        requires_gpu: bool,
    ) -> Self {
        ComputationalTask {
            name,
            description,
            priority,
            estimated_cpu_usage: estimated_cpu_usage.min(100),
            estimated_gpu_usage: estimated_gpu_usage.min(100),
            estimated_memory_usage,
            requires_gpu,
        }
    }

    /// Check if task can run on the given resource manager
    pub fn can_run(&self, resource_manager: &ResourceManager) -> bool {
        // If task requires GPU but GPU is not available, it can't run
        if self.requires_gpu && !resource_manager.gpu_available {
            return false;
        }

        // Check if priority allows running
        resource_manager.can_run_computation(self.priority)
    }

    /// Execute the task if resources allow
    pub fn execute<F, T>(
        &self,
        resource_manager: &ResourceManager,
        computation: F,
    ) -> Result<T, ResourceError>
    where
        F: FnOnce() -> T,
    {
        if !self.can_run(resource_manager) {
            return Err(ResourceError::ResourceLimitExceeded(format!(
                "Task '{}' cannot run with current resource limits",
                self.name
            )));
        }

        resource_manager.run_computation(computation, self.priority)
    }
}

/// A registry of different computational task types
pub struct ComputationalRegistry {
    pub tasks: Vec<ComputationalTask>,
}

impl ComputationalRegistry {
    /// Create a new computational registry
    pub fn new() -> Self {
        let default_tasks = vec![
            ComputationalTask::new(
                "Weather Analysis".to_string(),
                "Analyzes weather patterns and makes predictions".to_string(),
                ComputationPriority::Medium,
                60,    // CPU usage
                30,    // GPU usage
                512,   // Memory usage (MB)
                false, // Doesn't require GPU
            ),
            ComputationalTask::new(
                "Machine Learning Inference".to_string(),
                "Runs inference on pre-trained ML models".to_string(),
                ComputationPriority::High,
                50,   // CPU usage
                80,   // GPU usage
                1024, // Memory usage (MB)
                true, // Requires GPU
            ),
            ComputationalTask::new(
                "Blockchain Validation".to_string(),
                "Validates blockchain transactions".to_string(),
                ComputationPriority::Medium,
                70,    // CPU usage
                0,     // GPU usage
                256,   // Memory usage (MB)
                false, // Doesn't require GPU
            ),
            ComputationalTask::new(
                "Data Visualization".to_string(),
                "Generates visualizations from data".to_string(),
                ComputationPriority::Low,
                40,    // CPU usage
                20,    // GPU usage
                512,   // Memory usage (MB)
                false, // Doesn't require GPU
            ),
            ComputationalTask::new(
                "NFT Rendering".to_string(),
                "Renders graphical components of NFTs".to_string(),
                ComputationPriority::High,
                40,   // CPU usage
                90,   // GPU usage
                768,  // Memory usage (MB)
                true, // Requires GPU
            ),
        ];

        ComputationalRegistry {
            tasks: default_tasks,
        }
    }

    /// Add a task to the registry
    pub fn add_task(&mut self, task: ComputationalTask) {
        self.tasks.push(task);
    }

    /// Get a task by name
    pub fn get_task(&self, name: &str) -> Option<&ComputationalTask> {
        self.tasks.iter().find(|task| task.name == name)
    }

    /// Get all tasks
    pub fn get_all_tasks(&self) -> Vec<&ComputationalTask> {
        self.tasks.iter().collect()
    }

    /// Get tasks that can run with current resource settings
    pub fn get_runnable_tasks(
        &self,
        resource_manager: &ResourceManager,
    ) -> Vec<&ComputationalTask> {
        self.tasks
            .iter()
            .filter(|task| task.can_run(resource_manager))
            .collect()
    }
}

/// An atomic struct to represent currently running tasks
pub struct RunningTasks {
    weather_analysis: AtomicU32,
    ml_inference: AtomicU32,
    blockchain_validation: AtomicU32,
    data_visualization: AtomicU32,
    nft_rendering: AtomicU32,
}

impl RunningTasks {
    /// Create a new running tasks tracker
    pub fn new() -> Self {
        RunningTasks {
            weather_analysis: AtomicU32::new(0),
            ml_inference: AtomicU32::new(0),
            blockchain_validation: AtomicU32::new(0),
            data_visualization: AtomicU32::new(0),
            nft_rendering: AtomicU32::new(0),
        }
    }

    /// Increment the count for a task type
    pub fn increment(&self, task_name: &str) {
        match task_name {
            "Weather Analysis" => self.weather_analysis.fetch_add(1, Ordering::SeqCst),
            "Machine Learning Inference" => self.ml_inference.fetch_add(1, Ordering::SeqCst),
            "Blockchain Validation" => self.blockchain_validation.fetch_add(1, Ordering::SeqCst),
            "Data Visualization" => self.data_visualization.fetch_add(1, Ordering::SeqCst),
            "NFT Rendering" => self.nft_rendering.fetch_add(1, Ordering::SeqCst),
            _ => 0,
        };
    }

    /// Decrement the count for a task type
    pub fn decrement(&self, task_name: &str) {
        match task_name {
            "Weather Analysis" => {
                let val = self.weather_analysis.load(Ordering::SeqCst);
                if val > 0 {
                    self.weather_analysis.fetch_sub(1, Ordering::SeqCst);
                }
            },
            "Machine Learning Inference" => {
                let val = self.ml_inference.load(Ordering::SeqCst);
                if val > 0 {
                    self.ml_inference.fetch_sub(1, Ordering::SeqCst);
                }
            },
            "Blockchain Validation" => {
                let val = self.blockchain_validation.load(Ordering::SeqCst);
                if val > 0 {
                    self.blockchain_validation.fetch_sub(1, Ordering::SeqCst);
                }
            },
            "Data Visualization" => {
                let val = self.data_visualization.load(Ordering::SeqCst);
                if val > 0 {
                    self.data_visualization.fetch_sub(1, Ordering::SeqCst);
                }
            },
            "NFT Rendering" => {
                let val = self.nft_rendering.load(Ordering::SeqCst);
                if val > 0 {
                    self.nft_rendering.fetch_sub(1, Ordering::SeqCst);
                }
            },
            _ => {},
        }
    }

    /// Get the count for a task type
    pub fn get_count(&self, task_name: &str) -> u32 {
        match task_name {
            "Weather Analysis" => self.weather_analysis.load(Ordering::SeqCst),
            "Machine Learning Inference" => self.ml_inference.load(Ordering::SeqCst),
            "Blockchain Validation" => self.blockchain_validation.load(Ordering::SeqCst),
            "Data Visualization" => self.data_visualization.load(Ordering::SeqCst),
            "NFT Rendering" => self.nft_rendering.load(Ordering::SeqCst),
            _ => 0,
        }
    }

    /// Get total number of running tasks
    pub fn get_total(&self) -> u32 {
        self.weather_analysis.load(Ordering::SeqCst)
            + self.ml_inference.load(Ordering::SeqCst)
            + self.blockchain_validation.load(Ordering::SeqCst)
            + self.data_visualization.load(Ordering::SeqCst)
            + self.nft_rendering.load(Ordering::SeqCst)
    }
}

/// The main scheduler that manages computational tasks and resources
pub struct ComputationScheduler {
    resource_manager: Arc<ResourceManager>,
    task_registry: ComputationalRegistry,
    running_tasks: Arc<RunningTasks>,
    max_concurrent_tasks: usize,
}

impl ComputationScheduler {
    /// Create a new computation scheduler
    pub fn new(resource_manager: Arc<ResourceManager>) -> Self {
        ComputationScheduler {
            resource_manager,
            task_registry: ComputationalRegistry::new(),
            running_tasks: Arc::new(RunningTasks::new()),
            max_concurrent_tasks: 10, // Default max concurrent tasks
        }
    }

    /// Set the maximum number of concurrent tasks
    pub fn set_max_concurrent_tasks(&mut self, max: usize) {
        self.max_concurrent_tasks = max;
    }

    /// Get the maximum number of concurrent tasks
    pub fn get_max_concurrent_tasks(&self) -> usize {
        self.max_concurrent_tasks
    }

    /// Get the resource manager
    pub fn get_resource_manager(&self) -> Arc<ResourceManager> {
        self.resource_manager.clone()
    }

    /// Get the task registry
    pub fn get_task_registry(&self) -> &ComputationalRegistry {
        &self.task_registry
    }

    /// Get the running tasks
    pub fn get_running_tasks(&self) -> Arc<RunningTasks> {
        self.running_tasks.clone()
    }

    /// Submit a task for execution
    pub fn submit_task<F, T>(&self, task_name: &str, computation: F) -> Result<T, ResourceError>
    where
        F: FnOnce() -> T,
    {
        // Check if we're at the max concurrent tasks
        if self.running_tasks.get_total() as usize >= self.max_concurrent_tasks {
            return Err(ResourceError::ResourceLimitExceeded(format!(
                "Maximum concurrent tasks ({}) reached",
                self.max_concurrent_tasks
            )));
        }

        // Get the task
        let task = match self.task_registry.get_task(task_name) {
            Some(task) => task,
            None => {
                return Err(ResourceError::ResourceUnavailable(format!(
                    "Task '{}' not found in registry",
                    task_name
                )));
            },
        };

        // Check if task can run
        if !task.can_run(&self.resource_manager) {
            return Err(ResourceError::ResourceLimitExceeded(format!(
                "Task '{}' cannot run with current resource limits",
                task_name
            )));
        }

        // Increment the running tasks counter
        self.running_tasks.increment(task_name);

        // Execute the task
        let result = match task.execute(&self.resource_manager, computation) {
            Ok(res) => Ok(res),
            Err(e) => Err(e),
        };

        // Decrement the running tasks counter
        self.running_tasks.decrement(task_name);

        result
    }

    /// Get a summary of resource usage and tasks
    pub fn get_status_summary(&self) -> String {
        let usage = self.resource_manager.get_current_usage();
        let cpu_limit = self.resource_manager.get_cpu_limit();
        let gpu_limit = self.resource_manager.get_gpu_limit();
        let memory_limit = self.resource_manager.get_memory_limit();

        let mut summary = format!(
            "Resource Usage Summary:\n\
             - CPU: {}% (limit: {}%)\n\
             - GPU: {}% (limit: {}%)\n\
             - Memory: {} MB (limit: {} MB)\n\
             - Running Tasks: {}/{}\n\n\
             Task Breakdown:\n",
            usage.cpu_usage,
            cpu_limit,
            usage.gpu_usage,
            gpu_limit,
            usage.memory_usage,
            memory_limit,
            self.running_tasks.get_total(),
            self.max_concurrent_tasks
        );

        for task in self.task_registry.get_all_tasks() {
            let running_count = self.running_tasks.get_count(&task.name);
            let can_run = task.can_run(&self.resource_manager);

            summary.push_str(&format!(
                "- {} ({}): {} running, {} GPU\n  Priority: {:?}, CPU: {}%, Memory: {} MB\n  {}\n",
                task.name,
                if can_run { "available" } else { "unavailable" },
                running_count,
                if task.requires_gpu { "requires" } else { "no" },
                task.priority,
                task.estimated_cpu_usage,
                task.estimated_memory_usage,
                task.description
            ));
        }

        summary
    }
}
