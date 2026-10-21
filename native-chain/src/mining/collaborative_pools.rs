#![cfg(feature = "full-mining")]
//! Collaborative Mining Pools
//!
//! This module implements collaborative mining pools that allow users to form
//! specialized pools for particular computational tasks, sharing resources and rewards.
//!
//! Migrated from core4_src/mining/collaborative_pools.rs (issue #397).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use anyhow::{Result, anyhow};
use chrono::{DateTime, Utc};
use serde::{Serialize, Deserialize};
use log::{info, debug, warn};

// TODO(#960): Import Blockchain type once core4_src/blockchain/chain.rs is migrated
//   to a workspace crate. Needed for on-chain reward settlement in pools.
// use crate::blockchain::chain::Blockchain;

use crate::nft::ComputationalNft;

/// Persistence interface used by collaborative pool management.
/// Kept local so mining pools can run against multiple storage backends.
pub trait PoolStorage: Send + Sync {
    fn put(&self, key: &str, value: &[u8]) -> Result<()>;
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>>;
    fn find_keys_with_prefix(&self, prefix: &str) -> Result<Vec<String>>;
}

/// In-memory storage backend useful for tests and local bootstrap.
#[derive(Default)]
pub struct InMemoryPoolStorage {
    entries: Mutex<HashMap<String, Vec<u8>>>,
}

impl PoolStorage for InMemoryPoolStorage {
    fn put(&self, key: &str, value: &[u8]) -> Result<()> {
        let mut entries = self.entries.lock().map_err(|_| anyhow!("in-memory pool storage mutex poisoned"))?;
        entries.insert(key.to_string(), value.to_vec());
        Ok(())
    }

    fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        let entries = self.entries.lock().map_err(|_| anyhow!("in-memory pool storage mutex poisoned"))?;
        Ok(entries.get(key).cloned())
    }

    fn find_keys_with_prefix(&self, prefix: &str) -> Result<Vec<String>> {
        let entries = self.entries.lock().map_err(|_| anyhow!("in-memory pool storage mutex poisoned"))?;
        Ok(entries.keys().filter(|key| key.starts_with(prefix)).cloned().collect())
    }
}

static ID_COUNTER: AtomicU64 = AtomicU64::new(0);

fn generate_runtime_id(prefix: &str) -> String {
    let timestamp_nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let sequence = ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}-{timestamp_nanos}-{sequence}")
}

/// Specialization type for a mining pool
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PoolSpecialization {
    /// Weather forecasting and atmospheric science
    Weather,
    /// Climate modeling and research
    Climate,
    /// Wildfire risk assessment and prediction
    Wildfire,
    /// Air quality analysis and forecasting
    AirQuality,
    /// Ocean modeling and hydrology
    Ocean,
    /// Seismic analysis and earthquake prediction
    Seismic,
    /// Healthcare and epidemiological analysis
    Healthcare,
    /// Humanitarian crisis response and analysis
    Humanitarian,
    /// General purpose computation (non-specialized)
    General,
    /// Custom specialization with a name
    Custom(String),
}

impl std::fmt::Display for PoolSpecialization {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PoolSpecialization::Weather => write!(f, "Weather"),
            PoolSpecialization::Climate => write!(f, "Climate"),
            PoolSpecialization::Wildfire => write!(f, "Wildfire"),
            PoolSpecialization::AirQuality => write!(f, "Air Quality"),
            PoolSpecialization::Ocean => write!(f, "Ocean & Hydrology"),
            PoolSpecialization::Seismic => write!(f, "Seismic"),
            PoolSpecialization::Healthcare => write!(f, "Healthcare"),
            PoolSpecialization::Humanitarian => write!(f, "Humanitarian"),
            PoolSpecialization::General => write!(f, "General Purpose"),
            PoolSpecialization::Custom(name) => write!(f, "Custom: {}", name),
        }
    }
}

/// Simple voting type for pool governance
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Vote {
    /// Vote in favor
    Yes,
    /// Vote against
    No,
    /// Abstain from voting
    Abstain,
}

/// A simplified PublicKey type for our use
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PublicKey(pub String);

impl std::fmt::Display for PublicKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Default for PublicKey {
    fn default() -> Self {
        PublicKey("default_key".to_string())
    }
}

/// A simplified Signature type for our use
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Signature(pub Vec<u8>);

impl std::fmt::Display for Signature {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Sig_{:x?}", &self.0)
    }
}

/// Pool ownership and governance model
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PoolGovernance {
    /// One user has complete control
    SingleOwner {
        /// Owner's public key
        owner: PublicKey,
    },
    /// Democratic voting among members
    Democratic {
        /// Minimum votes required for a proposal to pass (percent)
        threshold: u8,
        /// Voting members
        members: Vec<PublicKey>,
    },
    /// Weighted voting based on contribution
    Weighted {
        /// Minimum weight required for a proposal to pass (percent)
        threshold: u8,
        /// Members with their voting weights
        members: HashMap<PublicKey, u32>,
    },
    /// Liquid democracy with delegation
    LiquidDemocracy {
        /// Delegations from member to delegate
        delegations: HashMap<PublicKey, PublicKey>,
        /// Base voting weight for each member
        base_weights: HashMap<PublicKey, u32>,
    },
}

/// Resource contribution by a pool member
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceContribution {
    /// Member public key
    pub member: PublicKey,
    /// CPU cores contributed
    pub cpu_cores: u16,
    /// GPU units contributed (normalized)
    pub gpu_units: u16,
    /// Memory in MB contributed
    pub memory_mb: u32,
    /// Storage in MB contributed
    pub storage_mb: u32,
    /// Network bandwidth in Mbps contributed
    pub bandwidth_mbps: u32,
    /// When this contribution was registered
    pub registered_at: DateTime<Utc>,
    /// Last heartbeat/verification time
    pub last_heartbeat: DateTime<Utc>,
    /// Special capabilities (CUDA, TPU, etc.)
    pub capabilities: Vec<String>,
}

/// Payout method for the pool
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PayoutMethod {
    /// Proportional to compute resources contributed
    ProportionalToResources,
    /// Equal division among all members
    EqualShares,
    /// Pay per task completed
    PayPerTask,
    /// Based on time contributed
    TimeBasedContribution,
    /// Custom formula
    Custom {
        /// Name of the custom formula
        name: String,
        /// Description of how the formula works
        description: String,
    },
}

/// Pool membership type
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MembershipType {
    /// Open for anyone to join
    Open,
    /// Requires approval by governance
    Approval,
    /// Requires an invite from existing members
    InviteOnly,
    /// Requires staking tokens to join
    Staking {
        /// Minimum amount to stake in tokens
        minimum_stake: u64,
    },
}

/// Proposal for pool governance
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolProposal {
    /// Unique proposal ID
    pub id: String,
    /// Proposal title
    pub title: String,
    /// Detailed description
    pub description: String,
    /// Who created the proposal
    pub creator: PublicKey,
    /// When it was created
    pub created_at: DateTime<Utc>,
    /// Deadline for voting
    pub deadline: DateTime<Utc>,
    /// Proposal type
    pub proposal_type: ProposalType,
    /// Current votes (member -> vote)
    pub votes: HashMap<PublicKey, Vote>,
    /// Execution status
    pub status: ProposalStatus,
}

/// Status of a proposal
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ProposalStatus {
    /// Currently accepting votes
    Active,
    /// Passed and executed
    Executed,
    /// Passed but not yet executed
    Approved,
    /// Failed to pass
    Rejected,
    /// Cancelled before deadline
    Cancelled,
}

/// Type of proposal for pool governance
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProposalType {
    /// Change pool parameters
    ChangeParameters {
        /// Updated pool parameters
        updated_params: HashMap<String, String>,
    },
    /// Add a new member
    AddMember {
        /// New member's public key
        new_member: PublicKey,
        /// Initial resource contribution
        initial_contribution: Option<ResourceContribution>,
    },
    /// Remove an existing member
    RemoveMember {
        /// Member to remove
        member: PublicKey,
        /// Reason for removal
        reason: String,
    },
    /// Change the pool's governance model
    ChangeGovernance {
        /// New governance model
        new_governance: PoolGovernance,
    },
    /// Change the pool's specialization
    ChangeSpecialization {
        /// New specialization
        new_specialization: PoolSpecialization,
    },
    /// Change the payout method
    ChangePayoutMethod {
        /// New payout method
        new_method: PayoutMethod,
    },
    /// Dissolve the pool entirely
    DissolvePool {
        /// How to distribute remaining funds
        distribution_method: String,
    },
    /// Change membership requirements
    ChangeMembership {
        /// New membership type
        new_type: MembershipType,
    },
    /// Free-form proposal for anything else
    Custom {
        /// Action data
        action_data: String,
    },
}

/// Statistics for a mining pool
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PoolStatistics {
    /// Total NFTs computed
    pub total_nfts_computed: u64,
    /// NFTs computed in the last 24 hours
    pub nfts_computed_24h: u64,
    /// Total computational time spent (seconds)
    pub total_computation_time_s: u64,
    /// Total rewards earned
    pub total_rewards: u64,
    /// Rewards earned in the last 24 hours
    pub rewards_24h: u64,
    /// Average accuracy score of computations
    pub average_accuracy: f64,
    /// Number of active members
    pub active_members: u32,
    /// Total resources contributed
    pub total_resources: ResourceMetrics,
    /// Average time to complete an NFT
    pub average_completion_time_s: u64,
    /// Specialization efficiency rating (0-100)
    pub specialization_efficiency: u8,
    /// Global pool rank (if applicable)
    pub global_rank: Option<u32>,
}

/// Resource contribution metrics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResourceMetrics {
    /// Total CPU cores
    pub total_cpu_cores: u32,
    /// Total GPU units
    pub total_gpu_units: u32,
    /// Total memory in MB
    pub total_memory_mb: u64,
    /// Total storage in MB
    pub total_storage_mb: u64,
    /// Total bandwidth in Mbps
    pub total_bandwidth_mbps: u64,
}

/// Computational job in the pool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolJob {
    /// Unique job ID
    pub id: String,
    /// Associated NFT ID
    pub nft_id: String,
    /// Current status
    pub status: JobStatus,
    /// Assigned member(s)
    pub assigned_to: Vec<PublicKey>,
    /// When job was created
    pub created_at: DateTime<Utc>,
    /// When job was assigned
    pub assigned_at: Option<DateTime<Utc>>,
    /// When job was completed
    pub completed_at: Option<DateTime<Utc>>,
    /// Estimated computation time (seconds)
    pub estimated_time_s: u64,
    /// Actual computation time (seconds)
    pub actual_time_s: Option<u64>,
    /// Rewards earned for this job
    pub rewards: Option<u64>,
    /// Priority level (1-10)
    pub priority: u8,
    /// Job-specific resource requirements
    pub resource_requirements: JobResourceRequirements,
    /// Verification status
    pub verification_status: Option<VerificationStatus>,
    /// Partial results for complex jobs
    pub partial_results: Vec<PartialResult>,
}

/// Status of a computational job
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobStatus {
    /// Waiting to be assigned
    Pending,
    /// Currently being processed
    InProgress,
    /// Successfully completed
    Completed,
    /// Failed to complete
    Failed,
    /// Cancelled before completion
    Cancelled,
    /// Waiting for verification
    PendingVerification,
    /// Declined by assigned member
    Declined,
    /// Partially completed
    PartiallyCompleted,
}

/// Verification status for a job
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum VerificationStatus {
    /// Job is verified and accepted
    Verified,
    /// Job verification failed
    Failed,
    /// Job is suspected of being incorrect
    Suspicious,
    /// Verification is in progress
    InProgress,
    /// Verification is pending
    Pending,
}

/// Job resource requirements
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobResourceRequirements {
    /// Minimum CPU cores needed
    pub min_cpu_cores: u16,
    /// Minimum GPU units needed
    pub min_gpu_units: u16,
    /// Minimum memory in MB needed
    pub min_memory_mb: u32,
    /// Minimum storage in MB needed
    pub min_storage_mb: u32,
    /// Minimum bandwidth in Mbps needed
    pub min_bandwidth_mbps: u32,
    /// Required capabilities
    pub required_capabilities: Vec<String>,
    /// Optional capabilities that improve performance
    pub optional_capabilities: Vec<String>,
}

/// Partial result for a complex job
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartialResult {
    /// Part index
    pub part_index: u32,
    /// Member who computed this part
    pub computed_by: PublicKey,
    /// Result hash
    pub result_hash: String,
    /// When this part was completed
    pub completed_at: DateTime<Utc>,
    /// Computation time for this part (seconds)
    pub computation_time_s: u64,
    /// Verification status
    pub verified: bool,
}

/// Member information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolMember {
    /// Member's public key
    pub public_key: PublicKey,
    /// Member's name or identifier
    pub name: String,
    /// When they joined
    pub joined_at: DateTime<Utc>,
    /// Current resource contribution
    pub resources: ResourceContribution,
    /// Current status
    pub status: MemberStatus,
    /// Reputation score (0-100)
    pub reputation: u8,
    /// Total rewards earned
    pub total_rewards: u64,
    /// Jobs completed successfully
    pub jobs_completed: u64,
    /// Jobs failed
    pub jobs_failed: u64,
    /// Current stake amount (if applicable)
    pub current_stake: Option<u64>,
    /// Special roles in the pool
    pub roles: Vec<String>,
    /// Specialization category
    pub specialization: Option<PoolSpecialization>,
}

/// Status of a pool member
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemberStatus {
    /// Active and contributing
    Active,
    /// Online but not accepting jobs
    Idle,
    /// Not connected
    Offline,
    /// Removed from pool
    Removed,
    /// Suspended due to issues
    Suspended,
    /// Pending approval
    PendingApproval,
}

/// Collaborative Mining Pool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollaborativeMiningPool {
    /// Unique pool ID
    pub id: String,
    /// Pool name
    pub name: String,
    /// Pool description
    pub description: String,
    /// When the pool was created
    pub created_at: DateTime<Utc>,
    /// Pool's computational specialization
    pub specialization: PoolSpecialization,
    /// Governance model
    pub governance: PoolGovernance,
    /// Member list
    pub members: HashMap<PublicKey, PoolMember>,
    /// Current jobs
    pub jobs: HashMap<String, PoolJob>,
    /// Pool statistics
    pub statistics: PoolStatistics,
    /// Payout method
    pub payout_method: PayoutMethod,
    /// Membership type
    pub membership_type: MembershipType,
    /// Active proposals
    pub proposals: HashMap<String, PoolProposal>,
    /// Minimum resource requirements to join
    pub min_requirements: Option<JobResourceRequirements>,
    /// Public metadata (discoverable)
    pub public_metadata: HashMap<String, String>,
    /// Private metadata (members only)
    pub private_metadata: HashMap<String, String>,
}

impl CollaborativeMiningPool {
    /// Create a new mining pool
    pub fn new(
        name: String,
        description: String,
        specialization: PoolSpecialization,
        governance: PoolGovernance,
        payout_method: PayoutMethod,
        membership_type: MembershipType,
        founder: PublicKey,
        founder_name: String,
        initial_resources: ResourceContribution,
    ) -> Self {
        let pool_id = generate_runtime_id("pool");
        let now = Utc::now();

        // Create the founder as the first member
        let founder_member = PoolMember {
            public_key: founder.clone(),
            name: founder_name,
            joined_at: now,
            resources: initial_resources,
            status: MemberStatus::Active,
            reputation: 100, // Start with perfect reputation
            total_rewards: 0,
            jobs_completed: 0,
            jobs_failed: 0,
            current_stake: None,
            roles: vec!["Founder".to_string()],
            specialization: Some(specialization.clone()),
        };

        let mut members = HashMap::new();
        members.insert(founder, founder_member);

        // Initialize an empty pool
        CollaborativeMiningPool {
            id: pool_id,
            name,
            description,
            created_at: now,
            specialization,
            governance,
            members,
            jobs: HashMap::new(),
            statistics: PoolStatistics::default(),
            payout_method,
            membership_type,
            proposals: HashMap::new(),
            min_requirements: None,
            public_metadata: HashMap::new(),
            private_metadata: HashMap::new(),
        }
    }

    /// Add a new member to the pool
    pub fn add_member(&mut self, member: PoolMember) -> Result<()> {
        if self.members.contains_key(&member.public_key) {
            return Err(anyhow!("Member already exists in pool"));
        }

        // Check if the member meets minimum requirements
        if let Some(req) = &self.min_requirements {
            let res = &member.resources;
            if res.cpu_cores < req.min_cpu_cores ||
               res.gpu_units < req.min_gpu_units ||
               res.memory_mb < req.min_memory_mb ||
               res.storage_mb < req.min_storage_mb ||
               res.bandwidth_mbps < req.min_bandwidth_mbps {
                return Err(anyhow!("Member does not meet minimum resource requirements"));
            }

            // Check for required capabilities
            for cap in &req.required_capabilities {
                if !res.capabilities.contains(cap) {
                    return Err(anyhow!("Member is missing required capability: {}", cap));
                }
            }
        }

        // Add the member
        self.members.insert(member.public_key.clone(), member);

        // Update statistics
        self.statistics.active_members = self.members.values()
            .filter(|m| m.status == MemberStatus::Active)
            .count() as u32;

        self.update_resource_metrics();

        Ok(())
    }

    /// Remove a member from the pool
    pub fn remove_member(&mut self, public_key: &PublicKey, reason: &str) -> Result<()> {
        let member = self.members.get_mut(public_key)
            .ok_or_else(|| anyhow!("Member not found in pool"))?;

        // Mark the member as removed
        member.status = MemberStatus::Removed;

        // Update statistics
        self.statistics.active_members = self.members.values()
            .filter(|m| m.status == MemberStatus::Active)
            .count() as u32;

        self.update_resource_metrics();

        // Reassign any active jobs from this member
        for (_, job) in self.jobs.iter_mut() {
            if job.status == JobStatus::InProgress && job.assigned_to.contains(public_key) {
                job.status = JobStatus::Pending;
                job.assigned_to.retain(|pk| pk != public_key);
                job.assigned_at = None;
            }
        }

        info!("Member {} removed from pool {}: {}", public_key, self.id, reason);

        Ok(())
    }

    /// Update the pool's resource metrics
    fn update_resource_metrics(&mut self) {
        let mut metrics = ResourceMetrics::default();

        for member in self.members.values() {
            if member.status == MemberStatus::Active {
                metrics.total_cpu_cores += member.resources.cpu_cores as u32;
                metrics.total_gpu_units += member.resources.gpu_units as u32;
                metrics.total_memory_mb += member.resources.memory_mb as u64;
                metrics.total_storage_mb += member.resources.storage_mb as u64;
                metrics.total_bandwidth_mbps += member.resources.bandwidth_mbps as u64;
            }
        }

        self.statistics.total_resources = metrics;
    }

    /// Submit a new computational job to the pool
    pub fn submit_job(&mut self, nft: &ComputationalNft) -> Result<String> {
        let job_id = generate_runtime_id("job");

        // Determine resource requirements from the NFT
        let requirements = self.estimate_job_requirements(nft)?;

        // Create the job
        let job = PoolJob {
            id: job_id.clone(),
            nft_id: nft.id.clone(),
            status: JobStatus::Pending,
            assigned_to: Vec::new(),
            created_at: Utc::now(),
            assigned_at: None,
            completed_at: None,
            estimated_time_s: self.estimate_computation_time(nft, &requirements)?,
            actual_time_s: None,
            rewards: None,
            priority: 5, // Default priority
            resource_requirements: requirements,
            verification_status: None,
            partial_results: Vec::new(),
        };

        // Add the job to the pool
        self.jobs.insert(job_id.clone(), job);

        info!("New job {} added to pool {} for NFT {}", job_id, self.id, nft.id);

        Ok(job_id)
    }

    /// Estimate resource requirements for a job
    fn estimate_job_requirements(&self, nft: &ComputationalNft) -> Result<JobResourceRequirements> {
        // Here we'd analyze the NFT's computation_type and data to determine requirements
        // This is a simplified version

        // Start with base requirements
        let mut requirements = JobResourceRequirements {
            min_cpu_cores: 2,
            min_gpu_units: 0,
            min_memory_mb: 1024,
            min_storage_mb: 512,
            min_bandwidth_mbps: 10,
            required_capabilities: Vec::new(),
            optional_capabilities: Vec::new(),
        };

        // Adjust based on NFT properties
        match nft.computation_type.as_ref() {
            "weather_model" | "climate_model" => {
                requirements.min_cpu_cores = 4;
                requirements.min_memory_mb = 4096;
                requirements.min_storage_mb = 2048;
                requirements.required_capabilities.push("SSE4.2".to_string());
            },
            "wildfire_prediction" | "air_quality_analysis" => {
                requirements.min_cpu_cores = 2;
                requirements.min_gpu_units = 1;
                requirements.min_memory_mb = 2048;
                requirements.required_capabilities.push("CUDA".to_string());
            },
            "hurricane_tracking" | "flood_model" => {
                requirements.min_cpu_cores = 8;
                requirements.min_memory_mb = 8192;
                requirements.min_storage_mb = 4096;
                requirements.required_capabilities.push("AVX2".to_string());
            },
            _ => {
                // Default requirements for unknown algorithms
            }
        }

        // GPU-specific adjustments
        if nft.computation_type.contains("prediction") || nft.computation_type.contains("analysis") {
            // For prediction or analysis tasks, assume GPU is needed
            requirements.min_gpu_units = 1;
            requirements.required_capabilities.push("CUDA".to_string());
            requirements.optional_capabilities.push("CUDA 11.0+".to_string());
        }

        // Data size adjustments
        let estimated_data_size = match nft.computation_type.as_ref() {
            "weather_model" | "air_quality_analysis" => 1024,
            "climate_model" | "hurricane_tracking" => 4096,
            "wildfire_prediction" => 2048,
            _ => 512,
        };

        requirements.min_storage_mb = requirements.min_storage_mb.max(estimated_data_size * 2);
        requirements.min_memory_mb = requirements.min_memory_mb.max(estimated_data_size);

        Ok(requirements)
    }

    /// Estimate computation time for a job
    fn estimate_computation_time(&self, nft: &ComputationalNft, _requirements: &JobResourceRequirements) -> Result<u64> {
        // Base estimate
        let mut estimated_seconds = 300u64; // 5 minutes default

        // Adjust based on computation_type complexity and data size
        match nft.computation_type.as_ref() {
            "weather_model" => estimated_seconds = 1800,
            "climate_model" => estimated_seconds = 3600,
            "wildfire_prediction" => estimated_seconds = 1200,
            "air_quality_analysis" => estimated_seconds = 900,
            "hurricane_tracking" => estimated_seconds = 2700,
            "seismic_analysis" => estimated_seconds = 1500,
            _ => {}
        }

        // Apply scale factors
        estimated_seconds = (estimated_seconds as f64 * 1.5) as u64;
        estimated_seconds = (estimated_seconds as f64 * 2.0) as u64;

        // Cap the estimate at reasonable bounds
        estimated_seconds = estimated_seconds.max(60).min(86400);

        Ok(estimated_seconds)
    }

    /// Assign jobs to pool members based on resources and specialization
    pub fn assign_jobs(&mut self) -> Result<Vec<String>> {
        let mut assigned_jobs = Vec::new();

        struct JobAssignment {
            job_id: String,
            member_key: PublicKey,
        }

        let mut assignments_to_make: Vec<JobAssignment> = Vec::new();

        // Get pending jobs
        let pending_jobs: Vec<String> = self.jobs.iter()
            .filter(|(_, job)| job.status == JobStatus::Pending)
            .map(|(id, _)| id.clone())
            .collect();

        // Get available members
        let available_members: Vec<PublicKey> = self.members.iter()
            .filter(|(_, member)| member.status == MemberStatus::Active)
            .map(|(pk, _)| pk.clone())
            .collect();

        if available_members.is_empty() {
            debug!("No available members in pool {}", self.id);
            return Ok(assigned_jobs);
        }

        // PHASE 1: Collect all data with immutable references
        {
            let mut job_data = Vec::new();

            for job_id in &pending_jobs {
                if let Some(job) = self.jobs.get(job_id) {
                    let requirements = job.resource_requirements.clone();
                    let computation_type = Self::infer_computation_type_from_requirements(&requirements);

                    job_data.push((job_id.clone(), requirements, computation_type));
                }
            }

            // Find suitable members for each job
            for (job_id, requirements, computation_type) in job_data {
                let mut suitable_members = Vec::new();

                for pk in &available_members {
                    if let Some(member) = self.members.get(pk) {
                        let res = &member.resources;

                        // Check base resource requirements
                        if res.cpu_cores < requirements.min_cpu_cores ||
                           res.gpu_units < requirements.min_gpu_units ||
                           res.memory_mb < requirements.min_memory_mb ||
                           res.storage_mb < requirements.min_storage_mb ||
                           res.bandwidth_mbps < requirements.min_bandwidth_mbps {
                            continue;
                        }

                        // Check required capabilities
                        let mut has_capabilities = true;
                        for cap in &requirements.required_capabilities {
                            if !res.capabilities.contains(cap) {
                                has_capabilities = false;
                                break;
                            }
                        }

                        if !has_capabilities {
                            continue;
                        }

                        // Check specialization match
                        if let Some(member_spec) = &member.specialization {
                            let algorithm_matches = Self::computation_type_matches_specialization(
                                &computation_type,
                                member_spec,
                            );

                            if algorithm_matches {
                                suitable_members.push(pk.clone());
                                break;
                            }
                        }

                        // If we didn't find a specialized match, member is still suitable
                        suitable_members.push(pk.clone());
                    }
                }

                // If we found suitable members, prepare the assignment
                if !suitable_members.is_empty() {
                    let member_key = suitable_members[0].clone();
                    assignments_to_make.push(JobAssignment {
                        job_id,
                        member_key,
                    });
                } else {
                    debug!("No suitable members found for job {} in pool {}", job_id, self.id);
                }
            }
        }

        // PHASE 2: Apply all assignments with mutable references
        for assignment in assignments_to_make {
            if let Some(job) = self.jobs.get_mut(&assignment.job_id) {
                job.status = JobStatus::InProgress;
                job.assigned_to = vec![assignment.member_key.clone()];
                job.assigned_at = Some(Utc::now());

                info!("Job {} assigned to member {} in pool {}",
                     assignment.job_id, assignment.member_key, self.id);
                assigned_jobs.push(assignment.job_id.clone());
            }
        }

        Ok(assigned_jobs)
    }

    fn infer_computation_type_from_requirements(requirements: &JobResourceRequirements) -> String {
        if requirements.min_gpu_units > 0 {
            "wildfire_prediction".to_string()
        } else if requirements.min_cpu_cores >= 8 {
            "climate_model".to_string()
        } else if requirements.min_cpu_cores >= 4 {
            "weather_model".to_string()
        } else {
            "unknown".to_string()
        }
    }

    fn computation_type_matches_specialization(
        computation_type: &str,
        member_spec: &PoolSpecialization,
    ) -> bool {
        match (computation_type, member_spec) {
            ("weather_model", PoolSpecialization::Weather) => true,
            ("climate_model", PoolSpecialization::Climate) => true,
            ("wildfire_prediction", PoolSpecialization::Wildfire) => true,
            ("air_quality_analysis", PoolSpecialization::AirQuality) => true,
            ("flood_model", PoolSpecialization::Ocean) => true,
            ("hurricane_tracking", PoolSpecialization::Weather) => true,
            ("seismic_analysis", PoolSpecialization::Seismic) => true,
            (_, PoolSpecialization::General) => true,
            _ => false,
        }
    }

    /// Complete a job and distribute rewards
    pub fn complete_job(&mut self, job_id: &str, _result: &str, computation_time_s: u64) -> Result<u64> {
        // Pre-calculate reward first
        let reward = {
            let job = self.jobs.get(job_id)
                .ok_or_else(|| anyhow!("Job not found in pool"))?;

            if job.status != JobStatus::InProgress {
                return Err(anyhow!("Job is not in progress"));
            }

            self.calculate_job_reward(job)
        };

        // Now update the job with mutable reference
        let job = self.jobs.get_mut(job_id)
            .ok_or_else(|| anyhow!("Job not found in pool"))?;

        // Mark job as completed
        job.status = JobStatus::Completed;
        job.completed_at = Some(Utc::now());
        job.actual_time_s = Some(computation_time_s);
        job.verification_status = Some(VerificationStatus::Pending);
        job.rewards = Some(reward);

        // Update pool statistics
        self.statistics.total_nfts_computed += 1;
        self.statistics.nfts_computed_24h += 1;
        self.statistics.total_computation_time_s += computation_time_s;
        self.statistics.total_rewards += reward;
        self.statistics.rewards_24h += reward;

        // Update member statistics
        for member_key in &job.assigned_to {
            if let Some(member) = self.members.get_mut(member_key) {
                member.jobs_completed += 1;
                member.total_rewards += reward;
            }
        }

        // Assume verification is automatic
        job.verification_status = Some(VerificationStatus::Verified);

        info!("Job {} completed in pool {}, reward: {}", job_id, self.id, reward);

        Ok(reward)
    }

    /// Calculate rewards for a completed job
    fn calculate_job_reward(&self, job: &PoolJob) -> u64 {
        let base_reward = 100u64;

        match &self.payout_method {
            PayoutMethod::ProportionalToResources => {
                let resource_factor = (job.resource_requirements.min_cpu_cores as f64 / 2.0 +
                                     job.resource_requirements.min_gpu_units as f64 +
                                     job.resource_requirements.min_memory_mb as f64 / 1024.0).max(1.0);

                (base_reward as f64 * resource_factor) as u64
            },
            PayoutMethod::EqualShares => {
                base_reward
            },
            PayoutMethod::PayPerTask => {
                let complexity_bonus = if let Some(time) = job.actual_time_s {
                    (time as f64 / 300.0).min(5.0)
                } else {
                    1.0
                };

                (base_reward as f64 * complexity_bonus) as u64
            },
            PayoutMethod::TimeBasedContribution => {
                if let Some(time) = job.actual_time_s {
                    base_reward + (time / 60) * 10
                } else {
                    base_reward
                }
            },
            PayoutMethod::Custom { .. } => {
                base_reward * 2
            },
        }
    }

    /// Submit a new governance proposal
    pub fn submit_proposal(&mut self, proposal: PoolProposal) -> Result<String> {
        if !self.members.contains_key(&proposal.creator) {
            return Err(anyhow!("Proposal creator is not a pool member"));
        }

        let proposal_id = proposal.id.clone();
        self.proposals.insert(proposal_id.clone(), proposal);

        info!("New proposal {} submitted to pool {}", proposal_id, self.id);

        Ok(proposal_id)
    }

    /// Vote on a governance proposal
    pub fn vote_on_proposal(&mut self, proposal_id: &str, voter: &PublicKey, vote: Vote) -> Result<()> {
        if !self.members.contains_key(voter) {
            return Err(anyhow!("Voter is not a pool member"));
        }

        let proposal = self.proposals.get_mut(proposal_id)
            .ok_or_else(|| anyhow!("Proposal not found"))?;

        if proposal.status != ProposalStatus::Active {
            return Err(anyhow!("Proposal is not active"));
        }

        if Utc::now() > proposal.deadline {
            proposal.status = ProposalStatus::Rejected;
            return Err(anyhow!("Voting deadline has passed"));
        }

        proposal.votes.insert(voter.clone(), vote);

        self.check_proposal_status(proposal_id)?;

        Ok(())
    }

    /// Check if a proposal has enough votes to pass or fail
    fn check_proposal_status(&mut self, proposal_id: &str) -> Result<()> {
        let proposal = self.proposals.get_mut(proposal_id)
            .ok_or_else(|| anyhow!("Proposal not found"))?;

        if proposal.status != ProposalStatus::Active {
            return Ok(());
        }

        match &self.governance {
            PoolGovernance::SingleOwner { owner } => {
                if let Some(vote) = proposal.votes.get(owner) {
                    proposal.status = if *vote == Vote::Yes {
                        ProposalStatus::Approved
                    } else {
                        ProposalStatus::Rejected
                    };
                }
            },
            PoolGovernance::Democratic { threshold, members } => {
                let mut yes_votes = 0;
                let mut no_votes = 0;

                for (voter, vote) in &proposal.votes {
                    if members.contains(voter) {
                        match vote {
                            Vote::Yes => yes_votes += 1,
                            Vote::No => no_votes += 1,
                            _ => {}
                        }
                    }
                }

                let total_votes = yes_votes + no_votes;
                let total_members = members.len();

                let yes_percentage = if total_members > 0 {
                    (yes_votes * 100) as u8 / total_members as u8
                } else {
                    0
                };

                let threshold = *threshold;
                if yes_percentage >= threshold {
                    proposal.status = ProposalStatus::Approved;
                } else if total_votes == total_members || Utc::now() > proposal.deadline {
                    proposal.status = ProposalStatus::Rejected;
                }
            },
            PoolGovernance::Weighted { threshold, members } => {
                let mut yes_weight = 0;
                let mut total_weight = 0;

                for (member, weight) in members {
                    total_weight += weight;

                    if let Some(vote) = proposal.votes.get(member) {
                        if *vote == Vote::Yes {
                            yes_weight += weight;
                        }
                    }
                }

                let yes_percentage = if total_weight > 0 {
                    (yes_weight * 100) as u8 / total_weight as u8
                } else {
                    0
                };

                let threshold = *threshold;
                if yes_percentage >= threshold {
                    proposal.status = ProposalStatus::Approved;
                } else if proposal.votes.len() == members.len() || Utc::now() > proposal.deadline {
                    proposal.status = ProposalStatus::Rejected;
                }
            },
            PoolGovernance::LiquidDemocracy { delegations, base_weights } => {
                let mut yes_weight = 0;
                let mut no_weight = 0;
                let mut total_weight = 0;

                for (member, base_weight) in base_weights {
                    total_weight += base_weight;

                    if let Some(vote) = proposal.votes.get(member) {
                        match vote {
                            Vote::Yes => yes_weight += base_weight,
                            Vote::No => no_weight += base_weight,
                            _ => {}
                        }
                    } else if let Some(delegate) = delegations.get(member) {
                        if let Some(delegate_vote) = proposal.votes.get(delegate) {
                            match delegate_vote {
                                Vote::Yes => yes_weight += base_weight,
                                Vote::No => no_weight += base_weight,
                                _ => {}
                            }
                        }
                    }
                }

                let yes_percentage = if total_weight > 0 {
                    (yes_weight * 100) as u8 / total_weight as u8
                } else {
                    0
                };

                if yes_percentage > 50 {
                    proposal.status = ProposalStatus::Approved;
                } else if (yes_weight + no_weight) == total_weight || Utc::now() > proposal.deadline {
                    proposal.status = ProposalStatus::Rejected;
                }
            }
        }

        // Execute approved proposals
        if proposal.status == ProposalStatus::Approved {
            self.execute_proposal(proposal_id)?;
        }

        Ok(())
    }

    /// Execute an approved proposal
    fn execute_proposal(&mut self, proposal_id: &str) -> Result<()> {
        let proposal_type = {
            let proposal = self.proposals.get(proposal_id)
                .ok_or_else(|| anyhow!("Proposal not found"))?;

            if proposal.status != ProposalStatus::Approved {
                return Err(anyhow!("Proposal is not approved"));
            }

            proposal.proposal_type.clone()
        };

        match &proposal_type {
            ProposalType::ChangeParameters { updated_params } => {
                for (key, value) in updated_params {
                    if key == "name" {
                        self.name = value.clone();
                    } else if key == "description" {
                        self.description = value.clone();
                    } else {
                        self.public_metadata.insert(key.clone(), value.clone());
                    }
                }
            },
            ProposalType::AddMember { new_member, initial_contribution } => {
                if !self.members.contains_key(new_member) {
                    let member = PoolMember {
                        public_key: new_member.clone(),
                        name: format!("Member {}", new_member),
                        joined_at: Utc::now(),
                        resources: initial_contribution.clone().unwrap_or_else(|| {
                            ResourceContribution {
                                member: new_member.clone(),
                                cpu_cores: 2,
                                gpu_units: 0,
                                memory_mb: 1024,
                                storage_mb: 1024,
                                bandwidth_mbps: 10,
                                registered_at: Utc::now(),
                                last_heartbeat: Utc::now(),
                                capabilities: Vec::new(),
                            }
                        }),
                        status: MemberStatus::Active,
                        reputation: 70,
                        total_rewards: 0,
                        jobs_completed: 0,
                        jobs_failed: 0,
                        current_stake: None,
                        roles: Vec::new(),
                        specialization: None,
                    };

                    self.members.insert(new_member.clone(), member);
                    self.statistics.active_members += 1;
                    self.update_resource_metrics();
                }
            },
            ProposalType::RemoveMember { member, reason } => {
                let member_key = member.clone();
                let reason_clone = reason.clone();

                if self.members.contains_key(&member_key) {
                    if let Some(member) = self.members.remove(&member_key) {
                        if member.status == MemberStatus::Active {
                            self.statistics.active_members = self.statistics.active_members.saturating_sub(1);
                        }

                        for (_, job) in self.jobs.iter_mut() {
                            if job.assigned_to.contains(&member_key) {
                                job.assigned_to.retain(|pk| pk != &member_key);

                                if job.assigned_to.is_empty() && job.status == JobStatus::InProgress {
                                    job.status = JobStatus::Pending;
                                    job.assigned_at = None;
                                }
                            }
                        }

                        match &mut self.governance {
                            PoolGovernance::Democratic { members, .. } => {
                                members.retain(|m| m != &member_key);
                            },
                            PoolGovernance::Weighted { members, .. } => {
                                members.remove(&member_key);
                            },
                            PoolGovernance::LiquidDemocracy { delegations, base_weights, .. } => {
                                base_weights.remove(&member_key);
                                delegations.remove(&member_key);
                                delegations.retain(|_, delegate| delegate != &member_key);
                            },
                            _ => {}
                        }

                        info!("Member {} removed from pool {}: {}",
                              member_key, self.id, reason_clone);

                        self.update_resource_metrics();
                    }
                }
            },
            ProposalType::ChangeGovernance { new_governance } => {
                self.governance = new_governance.clone();
            },
            ProposalType::ChangeSpecialization { new_specialization } => {
                self.specialization = new_specialization.clone();
            },
            ProposalType::ChangePayoutMethod { new_method } => {
                self.payout_method = new_method.clone();
            },
            ProposalType::DissolvePool { distribution_method } => {
                self.public_metadata.insert("status".to_string(), "dissolving".to_string());
                self.public_metadata.insert("dissolution_method".to_string(), distribution_method.clone());
            },
            ProposalType::ChangeMembership { new_type } => {
                self.membership_type = new_type.clone();
            },
            ProposalType::Custom { action_data } => {
                info!("Executing custom action: {}", action_data);
            }
        }

        if let Some(proposal) = self.proposals.get_mut(proposal_id) {
            proposal.status = ProposalStatus::Executed;
        }

        info!("Proposal {} executed in pool {}", proposal_id, self.id);

        Ok(())
    }

    /// Update a member's resource contribution
    pub fn update_member_resources(&mut self, public_key: &PublicKey, resources: ResourceContribution) -> Result<()> {
        let member = self.members.get_mut(public_key)
            .ok_or_else(|| anyhow!("Member not found in pool"))?;

        member.resources = resources;
        member.resources.last_heartbeat = Utc::now();

        self.update_resource_metrics();

        Ok(())
    }

    /// Check for inactive members and update their status
    pub fn check_inactive_members(&mut self, inactive_threshold: Duration) -> Result<Vec<PublicKey>> {
        let now = Utc::now();
        let mut inactive_members = Vec::new();

        for (public_key, member) in self.members.iter_mut() {
            if member.status == MemberStatus::Active {
                let last_heartbeat = member.resources.last_heartbeat;
                let duration_since_heartbeat = now.signed_duration_since(last_heartbeat);

                if duration_since_heartbeat.to_std().unwrap_or_default() > inactive_threshold {
                    member.status = MemberStatus::Offline;
                    inactive_members.push(public_key.clone());

                    for (_, job) in self.jobs.iter_mut() {
                        if job.status == JobStatus::InProgress && job.assigned_to.contains(public_key) {
                            job.status = JobStatus::Pending;
                            job.assigned_to.retain(|pk| pk != public_key);
                            job.assigned_at = None;
                        }
                    }
                }
            }
        }

        self.statistics.active_members = self.members.values()
            .filter(|m| m.status == MemberStatus::Active)
            .count() as u32;

        self.update_resource_metrics();

        if !inactive_members.is_empty() {
            info!("Found {} inactive members in pool {}", inactive_members.len(), self.id);
        }

        Ok(inactive_members)
    }

    /// Clean up old completed jobs
    pub fn cleanup_old_jobs(&mut self, age_threshold: Duration) -> Result<u32> {
        let now = Utc::now();
        let mut cleaned_jobs = 0;

        let old_job_ids: Vec<String> = self.jobs.iter()
            .filter(|(_, job)| {
                if let Some(completed_at) = job.completed_at {
                    if job.status == JobStatus::Completed {
                        let duration_since_completion = now.signed_duration_since(completed_at);
                        return duration_since_completion.to_std().unwrap_or_default() > age_threshold;
                    }
                }
                false
            })
            .map(|(id, _)| id.clone())
            .collect();

        for job_id in old_job_ids {
            self.jobs.remove(&job_id);
            cleaned_jobs += 1;
        }

        if cleaned_jobs > 0 {
            debug!("Cleaned up {} old jobs from pool {}", cleaned_jobs, self.id);
        }

        Ok(cleaned_jobs)
    }

    /// Get detailed pool statistics
    pub fn get_detailed_statistics(&self) -> PoolStatistics {
        self.statistics.clone()
    }

    /// Verify NFT computation result (simplified version)
    pub fn verify_computation(&mut self, job_id: &str) -> Result<bool> {
        let job = self.jobs.get_mut(job_id)
            .ok_or_else(|| anyhow!("Job not found in pool"))?;

        if job.status != JobStatus::Completed {
            return Err(anyhow!("Job is not completed"));
        }

        job.verification_status = Some(VerificationStatus::Verified);

        self.statistics.average_accuracy =
            (self.statistics.average_accuracy * (self.statistics.total_nfts_computed - 1) as f64 + 100.0)
            / self.statistics.total_nfts_computed as f64;

        for member_key in &job.assigned_to {
            if let Some(member) = self.members.get_mut(member_key) {
                member.reputation = (member.reputation as u16 + 1).min(100) as u8;
            }
        }

        Ok(true)
    }

    /// Distribute rewards to all members based on the pool's payout method
    pub fn distribute_rewards(&mut self, total_reward: u64) -> Result<HashMap<PublicKey, u64>> {
        let mut member_rewards = HashMap::new();

        let active_count = self.members.values()
            .filter(|m| m.status == MemberStatus::Active)
            .count();
        if active_count == 0 {
            warn!("distribute_rewards: 0 active members, {} tokens undistributed", total_reward);
            return Ok(member_rewards);
        }

        match &self.payout_method {
            PayoutMethod::ProportionalToResources => {
                let mut total_cores = 0;
                let mut total_gpu = 0;
                let mut total_memory = 0;

                for member in self.members.values() {
                    if member.status == MemberStatus::Active {
                        total_cores += member.resources.cpu_cores as u64;
                        total_gpu += member.resources.gpu_units as u64;
                        total_memory += member.resources.memory_mb as u64;
                    }
                }

                let total_resource_score = total_cores * 100 + total_gpu * 500 + total_memory / 100;

                if total_resource_score > 0 {
                    for (pk, member) in &mut self.members {
                        if member.status == MemberStatus::Active {
                            let member_score = member.resources.cpu_cores as u64 * 100 +
                                            member.resources.gpu_units as u64 * 500 +
                                            member.resources.memory_mb as u64 / 100;

                            let member_reward = (total_reward * member_score) / total_resource_score;
                            member_rewards.insert(pk.clone(), member_reward);
                            member.total_rewards += member_reward;
                        }
                    }
                }
            },
            PayoutMethod::EqualShares => {
                let active_count = self.members.values()
                    .filter(|m| m.status == MemberStatus::Active)
                    .count();

                if active_count > 0 {
                    let reward_per_member = total_reward / active_count as u64;

                    for (pk, member) in &mut self.members {
                        if member.status == MemberStatus::Active {
                            member_rewards.insert(pk.clone(), reward_per_member);
                            member.total_rewards += reward_per_member;
                        }
                    }
                }
            },
            PayoutMethod::PayPerTask => {
                let mut total_completed = 0;

                for member in self.members.values() {
                    if member.status == MemberStatus::Active {
                        total_completed += member.jobs_completed;
                    }
                }

                if total_completed > 0 {
                    for (pk, member) in &mut self.members {
                        if member.status == MemberStatus::Active && member.jobs_completed > 0 {
                            let member_reward = (total_reward * member.jobs_completed) / total_completed;
                            member_rewards.insert(pk.clone(), member_reward);
                            member.total_rewards += member_reward;
                        }
                    }
                }
            },
            PayoutMethod::TimeBasedContribution => {
                let mut total_score = 0;
                let mut member_scores = HashMap::new();

                for (pk, member) in &self.members {
                    if member.status == MemberStatus::Active {
                        let resource_factor = member.resources.cpu_cores as u64 +
                                           member.resources.gpu_units as u64 * 5;
                        let job_factor = member.jobs_completed.max(1);
                        let score = resource_factor * job_factor;

                        total_score += score;
                        member_scores.insert(pk.clone(), score);
                    }
                }

                if total_score > 0 {
                    for (pk, score) in member_scores {
                        let member_reward = (total_reward * score) / total_score;
                        member_rewards.insert(pk.clone(), member_reward);

                        if let Some(member) = self.members.get_mut(&pk) {
                            member.total_rewards += member_reward;
                        }
                    }
                }
            },
            PayoutMethod::Custom { name, .. } => {
                info!("Using custom payout method: {}", name);

                let mut total_reputation = 0;

                for member in self.members.values() {
                    if member.status == MemberStatus::Active {
                        total_reputation += member.reputation as u64;
                    }
                }

                if total_reputation > 0 {
                    for (pk, member) in &mut self.members {
                        if member.status == MemberStatus::Active {
                            let member_reward = (total_reward * member.reputation as u64) / total_reputation;
                            member_rewards.insert(pk.clone(), member_reward);
                            member.total_rewards += member_reward;
                        }
                    }
                }
            }
        }

        self.allocate_remainder(&mut member_rewards, total_reward);

        self.statistics.total_rewards += total_reward;
        self.statistics.rewards_24h += total_reward;

        Ok(member_rewards)
    }

    /// Allocate integer-division remainder to the pool founder (or first active member).
    /// Returns the number of extra tokens allocated.
    fn allocate_remainder(
        &mut self,
        member_rewards: &mut HashMap<PublicKey, u64>,
        total_reward: u64,
    ) -> u64 {
        let distributed: u64 = member_rewards.values().sum();
        let remainder = total_reward.saturating_sub(distributed);
        if remainder == 0 {
            return 0;
        }
        let recipient = self
            .members
            .iter()
            .find(|(_, m)| {
                m.status == MemberStatus::Active
                    && m.roles.contains(&"Founder".to_string())
            })
            .or_else(|| {
                self.members
                    .iter()
                    .find(|(_, m)| m.status == MemberStatus::Active)
            })
            .map(|(pk, _)| pk.clone());
        if let Some(pk) = recipient {
            *member_rewards.entry(pk.clone()).or_insert(0) += remainder;
            if let Some(member) = self.members.get_mut(&pk) {
                member.total_rewards += remainder;
            }
        }
        remainder
    }
}

/// Manager for collaborative mining pools
pub struct CollaborativePoolManager {
    /// All active pools
    pools: HashMap<String, CollaborativeMiningPool>,
    // TODO(#960): Add Blockchain field once the chain type is in the workspace.
    //   Needed for on-chain reward settlement and block-height gating.
    // blockchain: Arc<RwLock<Blockchain>>,
    /// Pool persistence backend
    storage: Arc<dyn PoolStorage>,
}

impl CollaborativePoolManager {
    /// Create a new collaborative pool manager
    pub fn new(storage: Arc<dyn PoolStorage>) -> Self {
        CollaborativePoolManager {
            pools: HashMap::new(),
            storage,
        }
    }

    /// Create a manager backed by in-memory storage.
    pub fn new_in_memory() -> Self {
        Self::new(Arc::new(InMemoryPoolStorage::default()))
    }

    /// Create a new mining pool
    pub fn create_pool(&mut self,
                      name: String,
                      description: String,
                      specialization: PoolSpecialization,
                      governance: PoolGovernance,
                      payout_method: PayoutMethod,
                      membership_type: MembershipType,
                      founder: PublicKey,
                      founder_name: String,
                      initial_resources: ResourceContribution) -> Result<String> {
        let name_for_log = name.clone();

        let pool = CollaborativeMiningPool::new(
            name,
            description,
            specialization,
            governance,
            payout_method,
            membership_type,
            founder,
            founder_name,
            initial_resources
        );

        let pool_id = pool.id.clone();

        self.pools.insert(pool_id.clone(), pool);

        self.save_pool(&pool_id)?;

        info!("New mining pool created: {} ({})", name_for_log, pool_id);

        Ok(pool_id)
    }

    /// Get a pool by ID
    pub fn get_pool(&self, pool_id: &str) -> Result<&CollaborativeMiningPool> {
        self.pools.get(pool_id)
            .ok_or_else(|| anyhow!("Pool not found: {}", pool_id))
    }

    /// Get a mutable reference to a pool by ID
    pub fn get_pool_mut(&mut self, pool_id: &str) -> Result<&mut CollaborativeMiningPool> {
        self.pools.get_mut(pool_id)
            .ok_or_else(|| anyhow!("Pool not found: {}", pool_id))
    }

    /// Get all pools
    pub fn get_all_pools(&self) -> Vec<&CollaborativeMiningPool> {
        self.pools.values().collect()
    }

    /// Find pools by specialization
    pub fn find_pools_by_specialization(&self, specialization: &PoolSpecialization) -> Vec<&CollaborativeMiningPool> {
        self.pools.values()
            .filter(|pool| &pool.specialization == specialization)
            .collect()
    }

    /// Find pools by member
    pub fn find_pools_by_member(&self, member_key: &PublicKey) -> Vec<&CollaborativeMiningPool> {
        self.pools.values()
            .filter(|pool| pool.members.contains_key(member_key))
            .collect()
    }

    /// Submit a job to the most appropriate pool
    pub fn submit_job_to_best_pool(&mut self, nft: &ComputationalNft) -> Result<(String, String)> {
        let pool_id = self.find_best_pool_for_nft(nft)?;

        let pool = self.get_pool_mut(&pool_id)?;

        let job_id = pool.submit_job(nft)?;

        self.save_pool(&pool_id)?;

        Ok((pool_id, job_id))
    }

    /// Find the best pool for a given NFT
    fn find_best_pool_for_nft(&self, nft: &ComputationalNft) -> Result<String> {
        let specialization = match nft.computation_type.as_str() {
            "weather_model" | "hurricane_tracking" => PoolSpecialization::Weather,
            "climate_model" => PoolSpecialization::Climate,
            "wildfire_prediction" => PoolSpecialization::Wildfire,
            "air_quality_analysis" => PoolSpecialization::AirQuality,
            "flood_model" | "ocean_current" => PoolSpecialization::Ocean,
            "seismic_analysis" => PoolSpecialization::Seismic,
            "epidemic_model" | "medical_imaging" => PoolSpecialization::Healthcare,
            "crisis_prediction" | "refugee_movement" => PoolSpecialization::Humanitarian,
            _ => PoolSpecialization::General,
        };

        let matching_pools: Vec<&CollaborativeMiningPool> = self.find_pools_by_specialization(&specialization);

        if matching_pools.is_empty() {
            let general_pools: Vec<&CollaborativeMiningPool> = self.find_pools_by_specialization(&PoolSpecialization::General);

            if general_pools.is_empty() {
                return Err(anyhow!("No suitable mining pools found for this NFT"));
            }

            let chosen_pool = general_pools.iter()
                .max_by_key(|p| p.statistics.total_resources.total_cpu_cores + p.statistics.total_resources.total_gpu_units * 10)
                .unwrap();

            return Ok(chosen_pool.id.clone());
        }

        let chosen_pool = matching_pools.iter()
            .max_by(|a, b| {
                let a_active = a.statistics.active_members;
                let b_active = b.statistics.active_members;

                if a_active != b_active {
                    return a_active.cmp(&b_active);
                }

                let a_resources = a.statistics.total_resources.total_cpu_cores +
                               a.statistics.total_resources.total_gpu_units * 10;
                let b_resources = b.statistics.total_resources.total_cpu_cores +
                               b.statistics.total_resources.total_gpu_units * 10;

                if a_resources != b_resources {
                    return a_resources.cmp(&b_resources);
                }

                a.statistics.average_completion_time_s.cmp(&b.statistics.average_completion_time_s).reverse()
            })
            .unwrap();

        Ok(chosen_pool.id.clone())
    }

    /// Join a mining pool
    pub fn join_pool(&mut self, pool_id: &str, member: PoolMember) -> Result<()> {
        let pool = self.get_pool_mut(pool_id)?;

        match &pool.membership_type {
            MembershipType::Open => {
                pool.add_member(member)?;
            },
            MembershipType::Approval => {
                let mut pending_member = member;
                pending_member.status = MemberStatus::PendingApproval;
                pool.add_member(pending_member)?;
            },
            MembershipType::InviteOnly => {
                return Err(anyhow!("This pool requires an invitation to join"));
            },
            MembershipType::Staking { minimum_stake } => {
                if let Some(staked) = member.current_stake {
                    if staked < *minimum_stake {
                        return Err(anyhow!("Insufficient stake: {} required, {} provided", minimum_stake, staked));
                    }
                    pool.add_member(member)?;
                } else {
                    return Err(anyhow!("This pool requires staking {} tokens", minimum_stake));
                }
            }
        }

        self.save_pool(pool_id)?;

        Ok(())
    }

    /// Distribute rewards across all pools
    pub fn distribute_global_rewards(&mut self, total_reward: u64) -> Result<()> {
        let mut total_active_members = 0;

        for pool in self.pools.values() {
            total_active_members += pool.statistics.active_members as u64;
        }

        if total_active_members == 0 {
            return Ok(());
        }

        for (pool_id, pool) in self.pools.iter_mut() {
            if pool.statistics.active_members > 0 {
                let pool_reward = (total_reward * pool.statistics.active_members as u64) / total_active_members;

                let member_rewards = pool.distribute_rewards(pool_reward)?;

                for (member_key, reward) in member_rewards {
                    info!("Member {} in pool {} received {} tokens", member_key, pool_id, reward);
                }
            }
        }

        Ok(())
    }

    /// Maintenance routine for all pools
    pub fn run_pool_maintenance(&mut self) -> Result<()> {
        let inactive_threshold = Duration::from_secs(3600);
        let old_job_threshold = Duration::from_secs(86400 * 7);

        let pool_ids: Vec<String> = self.pools.keys().cloned().collect();

        for pool_id in &pool_ids {
            {
                let pool = self.pools.get_mut(pool_id)
                    .ok_or_else(|| anyhow!("Pool not found: {}", pool_id))?;

                let inactive_members = pool.check_inactive_members(inactive_threshold)?;

                if !inactive_members.is_empty() {
                    debug!("Found {} inactive members in pool {}", inactive_members.len(), pool_id);
                }

                let cleaned_jobs = pool.cleanup_old_jobs(old_job_threshold)?;

                if cleaned_jobs > 0 {
                    debug!("Cleaned {} old jobs from pool {}", cleaned_jobs, pool_id);
                }

                let assigned_jobs = pool.assign_jobs()?;

                if !assigned_jobs.is_empty() {
                    debug!("Assigned {} jobs in pool {}", assigned_jobs.len(), pool_id);
                }
            }

            self.save_pool(pool_id)?;
        }

        Ok(())
    }

    /// Save pool data to persistent storage
    fn save_pool(&self, pool_id: &str) -> Result<()> {
        let pool = self.get_pool(pool_id)?;

        let pool_data = serde_json::to_vec(pool)?;

        self.storage.put(&format!("pool:{}", pool_id), &pool_data)?;

        Ok(())
    }

    /// Load pools from persistent storage
    pub fn load_pools(&mut self) -> Result<usize> {
        let pool_keys = self.storage.find_keys_with_prefix("pool:")?;

        for key in pool_keys {
            if let Some(pool_data) = self.storage.get(&key)? {
                if let Ok(pool) = serde_json::from_slice::<CollaborativeMiningPool>(&pool_data) {
                    self.pools.insert(pool.id.clone(), pool);
                }
            }
        }

        info!("Loaded {} collaborative mining pools", self.pools.len());

        Ok(self.pools.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_pool() -> CollaborativeMiningPool {
        let founder_key = PublicKey("founder-pk-001".to_string());
        let now = chrono::Utc::now();
        let resources = ResourceContribution {
            member: founder_key.clone(),
            cpu_cores: 4,
            gpu_units: 0,
            memory_mb: 8192,
            storage_mb: 1024,
            bandwidth_mbps: 100,
            registered_at: now,
            last_heartbeat: now,
            capabilities: Vec::new(),
        };
        let mut pool = CollaborativeMiningPool::new(
            "Test Pool".to_string(),
            "A test pool".to_string(),
            PoolSpecialization::General,
            PoolGovernance::SingleOwner { owner: founder_key.clone() },
            PayoutMethod::EqualShares,
            MembershipType::Open,
            founder_key,
            "Founder".to_string(),
            resources.clone(),
        );
        // Add two more members
        let member2 = PoolMember {
            public_key: PublicKey("member-pk-002".to_string()),
            name: "Member 2".to_string(),
            joined_at: now,
            resources: ResourceContribution {
                member: PublicKey("member-pk-002".to_string()),
                ..resources.clone()
            },
            status: MemberStatus::Active,
            jobs_completed: 0,
            jobs_failed: 0,
            total_rewards: 0,
            reputation: 100,
            current_stake: None,
            roles: Vec::new(),
            specialization: None,
        };
        let member3 = PoolMember {
            public_key: PublicKey("member-pk-003".to_string()),
            name: "Member 3".to_string(),
            joined_at: now,
            resources: ResourceContribution {
                member: PublicKey("member-pk-003".to_string()),
                ..resources.clone()
            },
            status: MemberStatus::Active,
            jobs_completed: 0,
            jobs_failed: 0,
            total_rewards: 0,
            reputation: 100,
            current_stake: None,
            roles: Vec::new(),
            specialization: None,
        };
        pool.members.insert(PublicKey("member-pk-002".to_string()), member2);
        pool.members.insert(PublicKey("member-pk-003".to_string()), member3);
        pool
    }

    #[test]
    fn test_distribute_rewards_remainder_goes_to_founder() {
        let mut pool = make_test_pool();
        pool.payout_method = PayoutMethod::EqualShares;

        let rewards = pool.distribute_rewards(100).unwrap();
        let total_distributed: u64 = rewards.values().sum();

        // 100 / 3 = 33 each, remainder 1 goes to founder
        assert_eq!(total_distributed, 100, "all tokens must be distributed");
        assert_eq!(
            *rewards.get(&PublicKey("founder-pk-001".to_string())).unwrap(),
            34,
            "founder gets 33 + 1 remainder"
        );
    }

    #[test]
    fn test_distribute_rewards_zero_active_members() {
        let mut pool = make_test_pool();
        // Deactivate all members
        for member in pool.members.values_mut() {
            member.status = MemberStatus::Offline;
        }

        let rewards = pool.distribute_rewards(100).unwrap();
        assert!(rewards.is_empty(), "no rewards distributed to inactive members");
    }

    #[test]
    fn test_distribute_rewards_evenly_divisible() {
        let mut pool = make_test_pool();
        pool.payout_method = PayoutMethod::EqualShares;

        let rewards = pool.distribute_rewards(99).unwrap();
        let total_distributed: u64 = rewards.values().sum();

        // 99 / 3 = 33 each, no remainder
        assert_eq!(total_distributed, 99);
        for &r in rewards.values() {
            assert_eq!(r, 33);
        }
    }
}
