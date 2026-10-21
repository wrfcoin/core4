//! Real-World Data Integration Hub
//!
//! This module implements a comprehensive data integration system that connects
//! to various external data sources and provides standardized interfaces for
//! computational NFTs to access and process this data.

use anyhow::{Result, anyhow};
use chrono::{DateTime, Utc};
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, RwLock};

// Re-export submodules
pub mod adapters;
pub mod providers;
pub mod schemas;
pub mod simple;
pub mod sources;

use schemas::{DataFormat, DataSchema, SchemaValidator};
// Export the simplified implementation
pub use simple::SimpleDataHub;

/// Represents a data source category
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DataCategory {
    /// Weather data (temperature, precipitation, wind, etc.)
    Weather,
    /// Seismological data (earthquakes, tremors, etc.)
    Seismic,
    /// Air quality measurements (AQI, pollutants, etc.)
    AirQuality,
    /// Environmental sensor data (various environmental measurements)
    Environmental,
    /// Acoustic sensor data including gunshot detection
    Acoustic,
    /// Economic indicators (indices, rates, etc.)
    Economic,
    /// Satellite imagery and derived products
    Satellite,
    /// Water-related data (rivers, oceans, etc.)
    Hydrological,
    /// Climate model outputs and predictions
    Climate,
    /// Wildfire risk and detection data
    Wildfire,
    /// Humanitarian crisis and response data
    Humanitarian,
    /// Healthcare and epidemiological data
    Health,
    /// Custom or miscellaneous data types
    Custom(String),
}

impl fmt::Display for DataCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DataCategory::Weather => write!(f, "Weather"),
            DataCategory::Seismic => write!(f, "Seismic"),
            DataCategory::AirQuality => write!(f, "Air Quality"),
            DataCategory::Environmental => write!(f, "Environmental"),
            DataCategory::Acoustic => write!(f, "Acoustic"),
            DataCategory::Economic => write!(f, "Economic"),
            DataCategory::Satellite => write!(f, "Satellite"),
            DataCategory::Hydrological => write!(f, "Hydrological"),
            DataCategory::Climate => write!(f, "Climate"),
            DataCategory::Wildfire => write!(f, "Wildfire"),
            DataCategory::Humanitarian => write!(f, "Humanitarian"),
            DataCategory::Health => write!(f, "Health"),
            DataCategory::Custom(name) => write!(f, "Custom: {}", name),
        }
    }
}

/// Represents a data source
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataSource {
    /// Unique identifier for the data source
    pub id: String,
    /// Human-readable name
    pub name: String,
    /// Detailed description of the data source
    pub description: String,
    /// Categories this source provides data for
    pub categories: Vec<DataCategory>,
    /// URL or endpoint for the data source
    pub endpoint: String,
    /// Format(s) the source provides data in
    pub formats: Vec<DataFormat>,
    /// API key or token requirement
    pub requires_auth: bool,
    /// Authentication method if applicable
    pub auth_method: Option<String>,
    /// When this source was added
    pub added_at: DateTime<Utc>,
    /// Last successful connection
    pub last_connected: Option<DateTime<Utc>>,
    /// Typical response time in ms
    pub avg_response_time_ms: Option<u32>,
    /// Rate limits if applicable
    pub rate_limit: Option<String>,
    /// Reliability score (0-100)
    pub reliability_score: u8,
    /// Available schemas for this source
    pub available_schemas: Vec<String>,
    /// Additional metadata
    pub metadata: HashMap<String, String>,
}

/// Represents a data provider organization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataProvider {
    /// Unique identifier for the provider
    pub id: String,
    /// Organization name
    pub name: String,
    /// Organization description
    pub description: String,
    /// Website URL
    pub website: String,
    /// Contact information
    pub contact: String,
    /// Data sources offered by this provider
    pub data_sources: Vec<String>,
    /// Whether the provider is official/verified
    pub is_verified: bool,
    /// When the provider was registered
    pub registered_at: DateTime<Utc>,
    /// Agreement/terms URL
    pub terms_url: Option<String>,
    /// Additional metadata
    pub metadata: HashMap<String, String>,
}

/// Data request parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataRequest {
    /// Request ID
    pub id: String,
    /// Source to request from
    pub source_id: String,
    /// Data category
    pub category: DataCategory,
    /// Time range start (if applicable)
    pub time_start: Option<DateTime<Utc>>,
    /// Time range end (if applicable)
    pub time_end: Option<DateTime<Utc>>,
    /// Geographical area (GeoJSON)
    pub geo_area: Option<String>,
    /// Specific parameters for the request
    pub parameters: HashMap<String, String>,
    /// Requested format
    pub format: DataFormat,
    /// Schema to validate against
    pub schema: Option<String>,
    /// Request timestamp
    pub timestamp: DateTime<Utc>,
    /// Requester ID (wallet/node)
    pub requester_id: String,
    /// Priority level (1-10)
    pub priority: u8,
    /// Custom attributes
    pub custom_attributes: HashMap<String, String>,
}

/// Data response from a source
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataResponse {
    /// Response ID (matching request)
    pub id: String,
    /// Source that provided the data
    pub source_id: String,
    /// Success or failure
    pub success: bool,
    /// Error message if failed
    pub error_message: Option<String>,
    /// Response timestamp
    pub timestamp: DateTime<Utc>,
    /// Data category
    pub category: DataCategory,
    /// Format of the data
    pub format: DataFormat,
    /// Schema used (if any)
    pub schema: Option<String>,
    /// The actual data payload
    pub data: Vec<u8>,
    /// Data size in bytes
    pub size_bytes: usize,
    /// Response time in ms
    pub response_time_ms: u64,
    /// Whether data was from cache
    pub from_cache: bool,
    /// Verification status
    pub verified: bool,
    /// Metadata about the response
    pub metadata: HashMap<String, String>,
}

/// Credential for authenticating with data sources
#[derive(Clone, Serialize, Deserialize)]
pub struct DataSourceCredential {
    /// Source this credential is for
    pub source_id: String,
    /// API key or token
    pub api_key: String,
    /// Additional auth parameters
    pub parameters: HashMap<String, String>,
    /// When credential expires
    pub expires_at: Option<DateTime<Utc>>,
    /// Owner of this credential
    pub owner_id: String,
}

// Don't include sensitive credentials in debug output
impl fmt::Debug for DataSourceCredential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DataSourceCredential")
            .field("source_id", &self.source_id)
            .field("api_key", &"[REDACTED]")
            .field("parameters", &self.parameters)
            .field("expires_at", &self.expires_at)
            .field("owner_id", &self.owner_id)
            .finish()
    }
}

/// Central manager for the Data Integration Hub
pub struct DataHub {
    /// Available data sources
    sources: Arc<RwLock<HashMap<String, DataSource>>>,
    /// Registered data providers
    providers: Arc<RwLock<HashMap<String, DataProvider>>>,
    /// Stored credentials for data sources
    credentials: Arc<RwLock<HashMap<String, DataSourceCredential>>>,
    /// Available schemas
    schemas: Arc<RwLock<HashMap<String, DataSchema>>>,
    /// Schema validators
    validators: Arc<RwLock<HashMap<String, Box<dyn SchemaValidator + Send + Sync>>>>,
    /// Cache of recent data responses
    response_cache: Arc<RwLock<HashMap<String, DataResponse>>>,
}

impl DataHub {
    /// Create a new DataHub instance
    pub fn new() -> Self {
        DataHub {
            sources: Arc::new(RwLock::new(HashMap::new())),
            providers: Arc::new(RwLock::new(HashMap::new())),
            credentials: Arc::new(RwLock::new(HashMap::new())),
            schemas: Arc::new(RwLock::new(HashMap::new())),
            validators: Arc::new(RwLock::new(HashMap::new())),
            response_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a new data source
    pub fn register_source(&self, source: DataSource) -> Result<()> {
        let mut sources = self
            .sources
            .write()
            .map_err(|_| anyhow!("Failed to acquire write lock on sources"))?;

        if sources.contains_key(&source.id) {
            return Err(anyhow!("Data source with ID {} already exists", source.id));
        }

        info!(
            "Registering new data source: {} ({})",
            source.name, source.id
        );
        sources.insert(source.id.clone(), source);
        Ok(())
    }

    /// Register a new data provider
    pub fn register_provider(&self, provider: DataProvider) -> Result<()> {
        let mut providers = self
            .providers
            .write()
            .map_err(|_| anyhow!("Failed to acquire write lock on providers"))?;

        if providers.contains_key(&provider.id) {
            return Err(anyhow!(
                "Data provider with ID {} already exists",
                provider.id
            ));
        }

        info!(
            "Registering new data provider: {} ({})",
            provider.name, provider.id
        );
        providers.insert(provider.id.clone(), provider);
        Ok(())
    }

    /// Store a credential for a data source
    pub fn add_credential(&self, credential: DataSourceCredential) -> Result<()> {
        let mut credentials = self
            .credentials
            .write()
            .map_err(|_| anyhow!("Failed to acquire write lock on credentials"))?;

        // Create a credential key combining source and owner
        let key = format!("{}:{}", credential.source_id, credential.owner_id);

        debug!(
            "Adding credential for source {} (owner: {})",
            credential.source_id, credential.owner_id
        );
        credentials.insert(key, credential);
        Ok(())
    }

    /// Retrieve a data source by ID
    pub fn get_source(&self, id: &str) -> Result<DataSource> {
        let sources = self
            .sources
            .read()
            .map_err(|_| anyhow!("Failed to acquire read lock on sources"))?;

        sources
            .get(id)
            .cloned()
            .ok_or_else(|| anyhow!("Data source with ID {} not found", id))
    }

    /// Retrieve a data provider by ID
    pub fn get_provider(&self, id: &str) -> Result<DataProvider> {
        let providers = self
            .providers
            .read()
            .map_err(|_| anyhow!("Failed to acquire read lock on providers"))?;

        providers
            .get(id)
            .cloned()
            .ok_or_else(|| anyhow!("Data provider with ID {} not found", id))
    }

    /// Retrieve a credential for a data source and owner
    pub fn get_credential(&self, source_id: &str, owner_id: &str) -> Result<DataSourceCredential> {
        let credentials = self
            .credentials
            .read()
            .map_err(|_| anyhow!("Failed to acquire read lock on credentials"))?;

        let key = format!("{}:{}", source_id, owner_id);

        credentials.get(&key).cloned().ok_or_else(|| {
            anyhow!(
                "Credential for source {} and owner {} not found",
                source_id,
                owner_id
            )
        })
    }

    /// List all data sources
    pub fn list_sources(&self) -> Result<Vec<DataSource>> {
        let sources = self
            .sources
            .read()
            .map_err(|_| anyhow!("Failed to acquire read lock on sources"))?;
        Ok(sources.values().cloned().collect())
    }

    /// List all data providers
    pub fn list_providers(&self) -> Result<Vec<DataProvider>> {
        let providers = self
            .providers
            .read()
            .map_err(|_| anyhow!("Failed to acquire read lock on providers"))?;
        Ok(providers.values().cloned().collect())
    }

    /// List sources by category
    pub fn list_sources_by_category(&self, category: &DataCategory) -> Result<Vec<DataSource>> {
        let sources = self
            .sources
            .read()
            .map_err(|_| anyhow!("Failed to acquire read lock on sources"))?;

        let matching_sources = sources
            .values()
            .filter(|source| source.categories.contains(category))
            .cloned()
            .collect();

        Ok(matching_sources)
    }

    /// Submit a data request and get response
    pub fn request_data(&self, request: DataRequest) -> Result<DataResponse> {
        info!(
            "Processing data request {} for source {} (category: {})",
            request.id, request.source_id, request.category
        );

        // Placeholder for now - in real implementation, this would:
        // 1. Validate the request
        // 2. Check cache for recent identical requests
        // 3. Check if we have credentials for the source
        // 4. Make the actual external API call
        // 5. Validate and process the response
        // 6. Cache the result
        // 7. Return the response

        // For now, we'll just return a mock response
        let response = DataResponse {
            id: request.id.clone(),
            source_id: request.source_id.clone(),
            success: true,
            error_message: None,
            timestamp: Utc::now(),
            category: request.category.clone(),
            format: request.format.clone(),
            schema: request.schema.clone(),
            data: Vec::new(), // Would contain actual data in real implementation
            size_bytes: 0,
            response_time_ms: 250,
            from_cache: false,
            verified: true,
            metadata: HashMap::new(),
        };

        // Add to cache
        let mut cache = self
            .response_cache
            .write()
            .map_err(|_| anyhow!("Failed to acquire write lock on response cache"))?;
        cache.insert(response.id.clone(), response.clone());

        Ok(response)
    }

    /// Register a new schema
    pub fn register_schema(
        &self,
        schema_id: String,
        schema: DataSchema,
        validator: Box<dyn SchemaValidator + Send + Sync>,
    ) -> Result<()> {
        {
            let mut schemas = self
                .schemas
                .write()
                .map_err(|_| anyhow!("Failed to acquire write lock on schemas"))?;
            schemas.insert(schema_id.clone(), schema);
        }

        {
            let mut validators = self
                .validators
                .write()
                .map_err(|_| anyhow!("Failed to acquire write lock on validators"))?;
            validators.insert(schema_id.clone(), validator);
        }

        Ok(())
    }

    /// Validate data against a schema
    pub fn validate_data(&self, schema_id: &str, data: &[u8]) -> Result<bool> {
        let validators = self
            .validators
            .read()
            .map_err(|_| anyhow!("Failed to acquire read lock on validators"))?;

        let validator = validators
            .get(schema_id)
            .ok_or_else(|| anyhow!("Schema validator for {} not found", schema_id))?;

        validator.validate(data)
    }
}

impl Default for DataHub {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn response_time_ms_no_u32_wrap() {
        // Duration exceeding u32::MAX ms (~49.7 days) must not wrap
        let large_ms: i64 = u32::MAX as i64 + 1_000;
        let clamped = large_ms.max(0) as u64;
        assert_eq!(clamped, large_ms as u64, "large duration must not wrap");

        // Negative duration (clock skew) must clamp to 0, not wrap
        let negative_ms: i64 = -500;
        let clamped_neg = negative_ms.max(0) as u64;
        assert_eq!(clamped_neg, 0, "negative duration must clamp to 0");
    }
}
