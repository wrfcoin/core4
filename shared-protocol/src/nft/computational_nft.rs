use crate::crypto::hash::{Hash, Hashable};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

/// Represents the complexity level of computation required for a computational NFT
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum ComputationComplexity {
    /// Simple computational tasks (e.g., basic image filtering)
    Low,
    /// Moderate complexity (e.g., simple AI inference)
    Medium,
    /// Complex computations (e.g., training small ML models)
    High,
    /// Very intensive computations (e.g., rendering, simulation)
    VeryHigh,
}

/// Represents a Computational NFT that can store data and computation specifications
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ComputationalNFT {
    /// Unique identifier for the NFT
    pub id: String,

    /// Name of the NFT
    pub name: String,

    /// Description of the NFT
    pub description: String,

    /// Address of the creator
    pub creator: String,

    /// URI to the content (IPFS or other decentralized storage)
    pub content_uri: String,

    /// Type of computation (e.g., "image-generation", "weather-forecast", "data-analysis")
    pub computation_type: String,

    /// Complexity level of computation (1-10)
    pub computation_complexity: u8,

    /// Required resources for computation
    pub resource_requirements: ResourceRequirements,

    /// Input data for the computation
    pub input_data: Option<String>,

    /// Execution parameters
    pub parameters: FxHashMap<String, String>,

    /// Creation timestamp
    pub created_at: DateTime<Utc>,

    /// Last modified timestamp
    pub updated_at: DateTime<Utc>,

    /// Whether the NFT has been verified by validators
    pub is_verified: bool,

    /// List of validators who verified this NFT
    pub verifiers: Vec<String>,

    /// Geographic region metadata
    pub region: Option<String>,

    /// Accuracy score (0.0 to 1.0) for the computation results
    pub accuracy: Option<f64>,

    /// Creator reputation score (0.0 to 1.0)
    pub creator_reputation: Option<f64>,

    /// Signatures from verifiers
    pub verification_signatures: Vec<String>,

    /// Execution result hash, if executed
    pub execution_result_hash: Option<Hash>,

    /// Metadata for additional properties
    pub metadata: FxHashMap<String, String>,
}

/// Resource requirements for computation
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResourceRequirements {
    /// Minimum CPU cores required
    pub min_cpu_cores: u8,

    /// Minimum RAM in MB
    pub min_ram_mb: u32,

    /// Whether GPU is required
    pub requires_gpu: bool,

    /// Minimum GPU memory in MB, if GPU is required
    pub min_gpu_memory_mb: Option<u32>,

    /// Estimated execution time in seconds
    pub estimated_execution_time: u32,

    /// Maximum allowed execution time in seconds
    pub max_execution_time: u32,
}

/// Parameters for creating a new `ComputationalNFT`.
pub struct NftParams {
    pub name: String,
    pub description: String,
    pub creator: String,
    pub content_uri: String,
    pub computation_type: String,
    pub computation_complexity: u8,
    pub resource_requirements: ResourceRequirements,
    pub input_data: Option<String>,
    pub parameters: FxHashMap<String, String>,
}

impl ComputationalNFT {
    /// Creates a new Computational NFT
    pub fn new(
        params: NftParams,
        input_data: Option<String>,
        parameters: FxHashMap<String, String>,
    ) -> Self {
        let id = format!("nft-{}-{}", Utc::now().timestamp(), params.creator);

        Self {
            id,
            name: params.name,
            description: params.description,
            creator: params.creator,
            content_uri: params.content_uri,
            computation_type: params.computation_type,
            computation_complexity: params.computation_complexity,
            resource_requirements: params.resource_requirements,
            input_data,
            parameters,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            is_verified: false,
            verifiers: Vec::new(),
            verification_signatures: Vec::new(),
            execution_result_hash: None,
            metadata: FxHashMap::default(),
            region: None,
            accuracy: None,
            creator_reputation: None,
        }
    }

    /// Add a verification from a validator
    pub fn add_verification(&mut self, validator: &str, signature: &str) -> Result<()> {
        // Check if this validator has already verified this NFT
        if self.verifiers.contains(&validator.to_string()) {
            return Err(anyhow::anyhow!(
                "Validator {validator} has already verified this NFT"
            ));
        }

        // Add verification
        self.verifiers.push(validator.to_string());
        self.verification_signatures.push(signature.to_string());

        // Mark as verified if we have at least 3 verifications
        if self.verifiers.len() >= 3 {
            self.is_verified = true;
        }

        self.updated_at = Utc::now();
        Ok(())
    }

    /// Sets the execution result hash
    pub fn set_execution_result(&mut self, result_hash: Hash) {
        self.execution_result_hash = Some(result_hash);
        self.updated_at = Utc::now();
    }

    /// Add or update metadata
    pub fn set_metadata(&mut self, key: &str, value: &str) {
        self.metadata.insert(key.to_string(), value.to_string());
        self.updated_at = Utc::now();
    }

    /// Serialize to JSON
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string(self).context("Failed to serialize NFT to JSON")
    }

    /// Deserialize from JSON
    pub fn from_json(json: &str) -> Result<Self> {
        serde_json::from_str(json).context("Failed to deserialize NFT from JSON")
    }
}

impl Hashable for ComputationalNFT {
    fn calculate_hash(&self) -> Hash {
        // First get bytes representation by serializing to JSON
        let bytes = self
            .to_json()
            .unwrap_or_else(|_| String::from("{}"))
            .into_bytes();

        // Then calculate the hash of those bytes
        crate::crypto::hash::hash_bytes(&bytes)
    }

    fn bytes(&self) -> Vec<u8> {
        // Serialize to JSON and convert to bytes
        self.to_json()
            .unwrap_or_else(|_| String::from("{}"))
            .into_bytes()
    }
}
