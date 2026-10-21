//! Data sources implementation module
//!
//! This module provides the connectors and handlers for interfacing
//! with various external data sources.

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use log::{debug, error, info, warn};
// The whole module is behind `data-hub` now (core4#2316), so this import needs
// no cfg of its own. It previously carried `#[cfg(feature = "data_hub")]` —
// underscore, a feature that was never declared — so the import was always
// compiled out and the two `reqwest::Client` uses below resolved through the
// unconditional dependency instead. Dead gate, removed rather than renamed.
use reqwest;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::time::Duration;

use super::{
    DataCategory, DataFormat, DataRequest, DataResponse, DataSource, DataSourceCredential,
};

/// Trait for data source connectors
#[async_trait]
pub trait DataSourceConnector: Send + Sync {
    /// Get the source ID
    fn source_id(&self) -> &str;

    /// Check if the connector is ready (i.e., has necessary credentials if required)
    fn is_ready(&self) -> bool;

    /// Test the connection to the data source
    async fn test_connection(&self) -> Result<bool>;

    /// Execute a data request against this source
    async fn execute_request(&self, request: &DataRequest) -> Result<DataResponse>;

    /// Get available data categories from this source
    fn get_available_categories(&self) -> Vec<DataCategory>;

    /// Get rate limiting information
    fn get_rate_limit_info(&self) -> Option<String>;
}

/// Base connector with common functionality
#[derive(Clone, Serialize, Deserialize)]
pub struct BaseConnector {
    /// Source this connector is for
    pub source: DataSource,
    /// Credential for authenticated sources.
    ///
    /// `skip_serializing` (NOT `skip`): `DataSourceCredential` holds an `api_key`
    /// and derives `Serialize`, so a JSON dump of a connector would otherwise emit
    /// the key in plaintext (#2129). Deserialization is deliberately preserved —
    /// `BaseConnector` is public and derives `Deserialize`, so credential-bearing
    /// input must keep loading as `Some(..)`; a full `skip` silently dropped it to
    /// `None` and broke authenticated connectors after deserialization (#2344
    /// review). Both directions are pinned by `credential_redaction_is_one_way` in
    /// `tests/no_secret_in_output.rs` (`--features data-hub`).
    #[serde(skip_serializing)]
    pub credential: Option<DataSourceCredential>,
    /// HTTP client
    #[serde(skip)]
    client: reqwest::Client,
    /// Last request time
    #[serde(skip)]
    last_request_time: Option<DateTime<Utc>>,
}

impl BaseConnector {
    /// Create a new base connector
    pub fn new(source: DataSource, credential: Option<DataSourceCredential>) -> Self {
        // Create a custom HTTP client with appropriate timeouts
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_default();

        BaseConnector {
            source,
            credential,
            client,
            last_request_time: None,
        }
    }

    /// Check if we have valid credentials
    pub fn has_valid_credentials(&self) -> bool {
        if !self.source.requires_auth {
            return true;
        }

        match &self.credential {
            Some(cred) => {
                // Check if credential has expired
                if let Some(expires_at) = cred.expires_at {
                    if expires_at < Utc::now() {
                        return false;
                    }
                }
                true
            },
            None => false,
        }
    }

    /// Make a GET request to the source's API
    pub async fn make_get_request<T: DeserializeOwned>(
        &mut self,
        endpoint: &str,
        params: &HashMap<String, String>,
    ) -> Result<T> {
        if !self.has_valid_credentials() {
            return Err(anyhow!(
                "Missing or invalid credentials for source: {}",
                self.source.id
            ));
        }

        let url = format!("{}{}", self.source.endpoint.trim_end_matches('/'), endpoint);

        // Build the request
        let mut req_builder = self.client.get(&url);

        // Add authentication if needed
        if self.source.requires_auth {
            if let Some(cred) = &self.credential {
                if let Some(auth_method) = &self.source.auth_method {
                    match auth_method.as_str() {
                        "API Key" => {
                            // Typically added as a query parameter or header
                            req_builder = req_builder.header("X-API-Key", &cred.api_key);
                        },
                        "OAuth 2.0" => {
                            // Typically added as a Bearer token
                            req_builder = req_builder
                                .header("Authorization", format!("Bearer {}", &cred.api_key));
                        },
                        "Basic Auth" => {
                            // Username and password
                            if let Some(username) = cred.parameters.get("username") {
                                req_builder = req_builder.basic_auth(username, Some(&cred.api_key));
                            }
                        },
                        _ => {
                            // Default to query parameter
                            let mut full_params = params.clone();
                            full_params.insert("api_key".to_string(), cred.api_key.clone());
                            req_builder = req_builder.query(&full_params);
                        },
                    }
                } else {
                    // Default to query parameter
                    let mut full_params = params.clone();
                    full_params.insert("api_key".to_string(), cred.api_key.clone());
                    req_builder = req_builder.query(&full_params);
                }
            }
        } else {
            // No auth needed, just add parameters
            req_builder = req_builder.query(&params);
        }

        // Make the request
        debug!("Making GET request to {}", url);
        self.last_request_time = Some(Utc::now());

        let response = req_builder.send().await?;

        // Check for success
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("HTTP error {}: {}", status, body));
        }

        // Parse response
        let data = response.json::<T>().await?;

        Ok(data)
    }

    /// Make a POST request to the source's API
    pub async fn make_post_request<T: DeserializeOwned>(
        &mut self,
        endpoint: &str,
        body: &HashMap<String, JsonValue>,
    ) -> Result<T> {
        if !self.has_valid_credentials() {
            return Err(anyhow!(
                "Missing or invalid credentials for source: {}",
                self.source.id
            ));
        }

        let url = format!("{}{}", self.source.endpoint.trim_end_matches('/'), endpoint);

        // Build the request
        let mut req_builder = self.client.post(&url);

        // Add authentication if needed
        if self.source.requires_auth {
            if let Some(cred) = &self.credential {
                if let Some(auth_method) = &self.source.auth_method {
                    match auth_method.as_str() {
                        "API Key" => {
                            req_builder = req_builder.header("X-API-Key", &cred.api_key);
                        },
                        "OAuth 2.0" => {
                            req_builder = req_builder
                                .header("Authorization", format!("Bearer {}", &cred.api_key));
                        },
                        "Basic Auth" => {
                            if let Some(username) = cred.parameters.get("username") {
                                req_builder = req_builder.basic_auth(username, Some(&cred.api_key));
                            }
                        },
                        _ => {
                            // Default behavior
                            let mut full_body = body.clone();
                            full_body.insert(
                                "api_key".to_string(),
                                JsonValue::String(cred.api_key.clone()),
                            );
                            req_builder = req_builder.json(&full_body);
                        },
                    }
                } else {
                    // Default behavior
                    let mut full_body = body.clone();
                    full_body.insert(
                        "api_key".to_string(),
                        JsonValue::String(cred.api_key.clone()),
                    );
                    req_builder = req_builder.json(&full_body);
                }
            }
        } else {
            // No auth needed
            req_builder = req_builder.json(&body);
        }

        // Make the request
        debug!("Making POST request to {}", url);
        self.last_request_time = Some(Utc::now());

        let response = req_builder.send().await?;

        // Check for success
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("HTTP error {}: {}", status, body));
        }

        // Parse response
        let data = response.json::<T>().await?;

        Ok(data)
    }

    /// Download binary data from the source's API
    pub async fn download_binary(
        &mut self,
        endpoint: &str,
        params: &HashMap<String, String>,
    ) -> Result<Vec<u8>> {
        if !self.has_valid_credentials() {
            return Err(anyhow!(
                "Missing or invalid credentials for source: {}",
                self.source.id
            ));
        }

        let url = format!("{}{}", self.source.endpoint.trim_end_matches('/'), endpoint);

        // Build the request
        let mut req_builder = self.client.get(&url);

        // Add authentication if needed
        if self.source.requires_auth {
            if let Some(cred) = &self.credential {
                if let Some(auth_method) = &self.source.auth_method {
                    match auth_method.as_str() {
                        "API Key" => {
                            req_builder = req_builder.header("X-API-Key", &cred.api_key);
                        },
                        "OAuth 2.0" => {
                            req_builder = req_builder
                                .header("Authorization", format!("Bearer {}", &cred.api_key));
                        },
                        "Basic Auth" => {
                            if let Some(username) = cred.parameters.get("username") {
                                req_builder = req_builder.basic_auth(username, Some(&cred.api_key));
                            }
                        },
                        _ => {
                            // Default to query parameter
                            let mut full_params = params.clone();
                            full_params.insert("api_key".to_string(), cred.api_key.clone());
                            req_builder = req_builder.query(&full_params);
                        },
                    }
                } else {
                    // Default to query parameter
                    let mut full_params = params.clone();
                    full_params.insert("api_key".to_string(), cred.api_key.clone());
                    req_builder = req_builder.query(&full_params);
                }
            }
        } else {
            // No auth needed, just add parameters
            req_builder = req_builder.query(&params);
        }

        // Make the request
        debug!("Downloading binary data from {}", url);
        self.last_request_time = Some(Utc::now());

        let response = req_builder.send().await?;

        // Check for success
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("HTTP error {}: {}", status, body));
        }

        // Get the bytes
        let data = response.bytes().await?;

        Ok(data.to_vec())
    }
}

/// NOAA Weather Service connector implementation
pub struct NoaaWeatherConnector {
    base: BaseConnector,
}

impl NoaaWeatherConnector {
    /// Create a new NOAA Weather Service connector
    pub fn new(source: DataSource, credential: Option<DataSourceCredential>) -> Self {
        NoaaWeatherConnector {
            base: BaseConnector::new(source, credential),
        }
    }
}

#[async_trait]
impl DataSourceConnector for NoaaWeatherConnector {
    fn source_id(&self) -> &str {
        &self.base.source.id
    }

    fn is_ready(&self) -> bool {
        self.base.has_valid_credentials()
    }

    async fn test_connection(&self) -> Result<bool> {
        // For NWS, we can just check if the base endpoint is accessible
        let mut base = self.base.clone();
        let params = HashMap::new();

        match base.make_get_request::<JsonValue>("", &params).await {
            Ok(_) => Ok(true),
            Err(e) => {
                warn!("Failed to connect to NOAA NWS: {}", e);
                Ok(false)
            },
        }
    }

    async fn execute_request(&self, request: &DataRequest) -> Result<DataResponse> {
        // Validate request
        if request.source_id != self.base.source.id {
            return Err(anyhow!("Invalid source ID in request"));
        }

        let start_time = Utc::now();
        let mut params = HashMap::new();

        // Build parameters based on request
        if let Some(start) = &request.time_start {
            params.insert("start".to_string(), start.to_rfc3339());
        }

        if let Some(end) = &request.time_end {
            params.insert("end".to_string(), end.to_rfc3339());
        }

        // Add any custom parameters
        for (k, v) in &request.parameters {
            params.insert(k.clone(), v.clone());
        }

        // Determine endpoint based on category
        let endpoint = match request.category {
            DataCategory::Weather => "points",
            DataCategory::Climate => "products/climate",
            _ => return Err(anyhow!("Unsupported data category for NOAA NWS")),
        };

        // Execute the request
        let mut base = self.base.clone();
        let result: Result<JsonValue> = base.make_get_request(&endpoint, &params).await;

        match result {
            Ok(data) => {
                // Convert to requested format
                let response_data = match request.format {
                    DataFormat::JSON => serde_json::to_vec(&data)?,
                    DataFormat::XML => {
                        // Would convert JSON to XML here
                        return Err(anyhow!("XML format not currently supported for NOAA NWS"));
                    },
                    _ => return Err(anyhow!("Unsupported format for NOAA NWS")),
                };

                let end_time = Utc::now();
                let response_time_ms = (end_time - start_time).num_milliseconds().max(0) as u64;
                let size_bytes = response_data.len();

                // Build response
                let response = DataResponse {
                    id: request.id.clone(),
                    source_id: self.base.source.id.clone(),
                    success: true,
                    error_message: None,
                    timestamp: Utc::now(),
                    category: request.category.clone(),
                    format: request.format.clone(),
                    schema: request.schema.clone(),
                    data: response_data,
                    size_bytes,
                    response_time_ms,
                    from_cache: false,
                    verified: true,
                    metadata: HashMap::new(),
                };

                Ok(response)
            },
            Err(e) => {
                let end_time = Utc::now();
                let response_time_ms = (end_time - start_time).num_milliseconds().max(0) as u64;

                // Build error response
                let response = DataResponse {
                    id: request.id.clone(),
                    source_id: self.base.source.id.clone(),
                    success: false,
                    error_message: Some(e.to_string()),
                    timestamp: Utc::now(),
                    category: request.category.clone(),
                    format: request.format.clone(),
                    schema: request.schema.clone(),
                    data: Vec::new(),
                    size_bytes: 0,
                    response_time_ms,
                    from_cache: false,
                    verified: false,
                    metadata: HashMap::new(),
                };

                Ok(response)
            },
        }
    }

    fn get_available_categories(&self) -> Vec<DataCategory> {
        self.base.source.categories.clone()
    }

    fn get_rate_limit_info(&self) -> Option<String> {
        self.base.source.rate_limit.clone()
    }
}

/// USGS Earthquake connector implementation
pub struct UsgsEarthquakeConnector {
    base: BaseConnector,
}

impl UsgsEarthquakeConnector {
    /// Create a new USGS Earthquake connector
    pub fn new(source: DataSource, credential: Option<DataSourceCredential>) -> Self {
        UsgsEarthquakeConnector {
            base: BaseConnector::new(source, credential),
        }
    }
}

#[async_trait]
impl DataSourceConnector for UsgsEarthquakeConnector {
    fn source_id(&self) -> &str {
        &self.base.source.id
    }

    fn is_ready(&self) -> bool {
        self.base.has_valid_credentials()
    }

    async fn test_connection(&self) -> Result<bool> {
        // For USGS, we'll test with a simple query
        let mut base = self.base.clone();
        let mut params = HashMap::new();
        params.insert("format".to_string(), "geojson".to_string());
        params.insert("limit".to_string(), "1".to_string());

        match base.make_get_request::<JsonValue>("query", &params).await {
            Ok(_) => Ok(true),
            Err(e) => {
                warn!("Failed to connect to USGS Earthquake: {}", e);
                Ok(false)
            },
        }
    }

    async fn execute_request(&self, request: &DataRequest) -> Result<DataResponse> {
        // Validate request
        if request.source_id != self.base.source.id {
            return Err(anyhow!("Invalid source ID in request"));
        }

        let start_time = Utc::now();
        let mut params = HashMap::new();

        // Basic parameters
        params.insert("format".to_string(), "geojson".to_string());

        // Time range
        if let Some(start) = &request.time_start {
            params.insert("starttime".to_string(), start.to_rfc3339());
        }

        if let Some(end) = &request.time_end {
            params.insert("endtime".to_string(), end.to_rfc3339());
        }

        // Geographic area
        if let Some(geo) = &request.geo_area {
            // In GeoJSON we might have to extract the bounding box
            // This is a simplified version
            params.insert("bbox".to_string(), geo.clone());
        }

        // Add any custom parameters
        for (k, v) in &request.parameters {
            params.insert(k.clone(), v.clone());
        }

        // Execute the request
        let mut base = self.base.clone();
        let result: Result<JsonValue> = base.make_get_request("query", &params).await;

        match result {
            Ok(data) => {
                // Convert to requested format
                let response_data = match request.format {
                    DataFormat::JSON | DataFormat::GeoJSON => serde_json::to_vec(&data)?,
                    DataFormat::CSV => {
                        return Err(anyhow!(
                            "CSV format not currently supported for USGS Earthquake"
                        ));
                    },
                    _ => return Err(anyhow!("Unsupported format for USGS Earthquake")),
                };

                let end_time = Utc::now();
                let response_time_ms = (end_time - start_time).num_milliseconds().max(0) as u64;
                let size_bytes = response_data.len();

                // Build response
                let response = DataResponse {
                    id: request.id.clone(),
                    source_id: self.base.source.id.clone(),
                    success: true,
                    error_message: None,
                    timestamp: Utc::now(),
                    category: request.category.clone(),
                    format: request.format.clone(),
                    schema: request.schema.clone(),
                    data: response_data,
                    size_bytes,
                    response_time_ms,
                    from_cache: false,
                    verified: true,
                    metadata: HashMap::new(),
                };

                Ok(response)
            },
            Err(e) => {
                let end_time = Utc::now();
                let response_time_ms = (end_time - start_time).num_milliseconds().max(0) as u64;

                // Build error response
                let response = DataResponse {
                    id: request.id.clone(),
                    source_id: self.base.source.id.clone(),
                    success: false,
                    error_message: Some(e.to_string()),
                    timestamp: Utc::now(),
                    category: request.category.clone(),
                    format: request.format.clone(),
                    schema: request.schema.clone(),
                    data: Vec::new(),
                    size_bytes: 0,
                    response_time_ms,
                    from_cache: false,
                    verified: false,
                    metadata: HashMap::new(),
                };

                Ok(response)
            },
        }
    }

    fn get_available_categories(&self) -> Vec<DataCategory> {
        self.base.source.categories.clone()
    }

    fn get_rate_limit_info(&self) -> Option<String> {
        self.base.source.rate_limit.clone()
    }
}

/// Factory function to create the appropriate connector for a data source
pub fn create_connector(
    source: DataSource,
    credential: Option<DataSourceCredential>,
) -> Box<dyn DataSourceConnector> {
    match source.id.as_str() {
        "noaa-nws" => Box::new(NoaaWeatherConnector::new(source, credential)),
        "usgs-earthquake" => Box::new(UsgsEarthquakeConnector::new(source, credential)),
        // Other sources would be added here
        _ => Box::new(GenericConnector::new(source, credential)),
    }
}

/// Generic connector for sources without specialized implementations
pub struct GenericConnector {
    base: BaseConnector,
}

impl GenericConnector {
    /// Create a new generic connector
    pub fn new(source: DataSource, credential: Option<DataSourceCredential>) -> Self {
        GenericConnector {
            base: BaseConnector::new(source, credential),
        }
    }
}

#[async_trait]
impl DataSourceConnector for GenericConnector {
    fn source_id(&self) -> &str {
        &self.base.source.id
    }

    fn is_ready(&self) -> bool {
        self.base.has_valid_credentials()
    }

    async fn test_connection(&self) -> Result<bool> {
        let mut base = self.base.clone();
        let params = HashMap::new();

        match base.make_get_request::<JsonValue>("", &params).await {
            Ok(_) => Ok(true),
            Err(e) => {
                warn!("Failed to connect to {}: {}", self.base.source.id, e);
                Ok(false)
            },
        }
    }

    async fn execute_request(&self, request: &DataRequest) -> Result<DataResponse> {
        // Validate request
        if request.source_id != self.base.source.id {
            return Err(anyhow!("Invalid source ID in request"));
        }

        let start_time = Utc::now();
        let mut params = HashMap::new();

        // Build parameters based on request
        if let Some(start) = &request.time_start {
            params.insert("start".to_string(), start.to_rfc3339());
        }

        if let Some(end) = &request.time_end {
            params.insert("end".to_string(), end.to_rfc3339());
        }

        // Add any custom parameters
        for (k, v) in &request.parameters {
            params.insert(k.clone(), v.clone());
        }

        // Execute the request - for generic connector, we'll use the endpoint from the parameters
        let endpoint = request
            .parameters
            .get("endpoint")
            .cloned()
            .unwrap_or_default();
        let mut base = self.base.clone();
        let result: Result<JsonValue> = base.make_get_request(&endpoint, &params).await;

        match result {
            Ok(data) => {
                // Convert to requested format
                let response_data = match request.format {
                    DataFormat::JSON | DataFormat::GeoJSON => serde_json::to_vec(&data)?,
                    _ => return Err(anyhow!("Unsupported format for generic connector")),
                };

                let end_time = Utc::now();
                let response_time_ms = (end_time - start_time).num_milliseconds().max(0) as u64;
                let size_bytes = response_data.len();

                // Build response
                let response = DataResponse {
                    id: request.id.clone(),
                    source_id: self.base.source.id.clone(),
                    success: true,
                    error_message: None,
                    timestamp: Utc::now(),
                    category: request.category.clone(),
                    format: request.format.clone(),
                    schema: request.schema.clone(),
                    data: response_data,
                    size_bytes,
                    response_time_ms,
                    from_cache: false,
                    verified: true,
                    metadata: HashMap::new(),
                };

                Ok(response)
            },
            Err(e) => {
                let end_time = Utc::now();
                let response_time_ms = (end_time - start_time).num_milliseconds().max(0) as u64;

                // Build error response
                let response = DataResponse {
                    id: request.id.clone(),
                    source_id: self.base.source.id.clone(),
                    success: false,
                    error_message: Some(e.to_string()),
                    timestamp: Utc::now(),
                    category: request.category.clone(),
                    format: request.format.clone(),
                    schema: request.schema.clone(),
                    data: Vec::new(),
                    size_bytes: 0,
                    response_time_ms,
                    from_cache: false,
                    verified: false,
                    metadata: HashMap::new(),
                };

                Ok(response)
            },
        }
    }

    fn get_available_categories(&self) -> Vec<DataCategory> {
        self.base.source.categories.clone()
    }

    fn get_rate_limit_info(&self) -> Option<String> {
        self.base.source.rate_limit.clone()
    }
}
