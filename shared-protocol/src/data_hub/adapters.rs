//! Data adapters module
//!
//! This module provides adapters for transforming data between different formats and schemas,
//! making it possible to process data from different sources in a consistent way.

use anyhow::{Result, anyhow};
use log::{debug, error, info, warn};
use serde_json::Value as JsonValue;
use std::collections::HashMap;

use super::{DataCategory, DataFormat};

/// Represents a data transformation adapter
pub trait DataAdapter: Send + Sync {
    /// Get the source format
    fn source_format(&self) -> DataFormat;

    /// Get the target format
    fn target_format(&self) -> DataFormat;

    /// Transform data from source format to target format
    fn transform(&self, data: &[u8]) -> Result<Vec<u8>>;

    /// Check if this adapter can handle the given data
    fn can_handle(&self, category: &DataCategory, data: &[u8]) -> bool;
}

/// JSON to CSV adapter
pub struct JsonToCsvAdapter {
    /// Field mappings from JSON path to CSV column
    field_mappings: HashMap<String, String>,
    /// Categories this adapter can handle
    supported_categories: Vec<DataCategory>,
}

impl JsonToCsvAdapter {
    /// Create a new JSON to CSV adapter
    pub fn new() -> Self {
        let mut field_mappings = HashMap::new();
        field_mappings.insert("$.timestamp".to_string(), "timestamp".to_string());
        field_mappings.insert("$.location.latitude".to_string(), "latitude".to_string());
        field_mappings.insert("$.location.longitude".to_string(), "longitude".to_string());

        JsonToCsvAdapter {
            field_mappings,
            supported_categories: vec![
                DataCategory::Weather,
                DataCategory::Seismic,
                DataCategory::AirQuality,
                DataCategory::Environmental,
            ],
        }
    }

    /// Create a new JSON to CSV adapter with custom mappings
    pub fn with_mappings(field_mappings: HashMap<String, String>) -> Self {
        JsonToCsvAdapter {
            field_mappings,
            supported_categories: vec![
                DataCategory::Weather,
                DataCategory::Seismic,
                DataCategory::AirQuality,
                DataCategory::Environmental,
            ],
        }
    }

    /// Add support for a category
    pub fn add_category(&mut self, category: DataCategory) {
        if !self.supported_categories.contains(&category) {
            self.supported_categories.push(category);
        }
    }
}

impl DataAdapter for JsonToCsvAdapter {
    fn source_format(&self) -> DataFormat {
        DataFormat::JSON
    }

    fn target_format(&self) -> DataFormat {
        DataFormat::CSV
    }

    fn transform(&self, data: &[u8]) -> Result<Vec<u8>> {
        // Parse JSON data
        let json = serde_json::from_slice::<JsonValue>(data)
            .map_err(|e| anyhow!("Failed to parse JSON: {}", e))?;

        // Check if we have an array or a single object
        let records = if let Some(arr) = json.as_array() {
            arr.clone()
        } else {
            vec![json]
        };

        if records.is_empty() {
            return Ok("".into());
        }

        // Collect all field names from mappings
        let headers: Vec<String> = self.field_mappings.values().cloned().collect();

        // Create CSV writer
        let mut wtr = csv::WriterBuilder::new()
            .has_headers(true)
            .from_writer(Vec::new());

        // Write headers
        wtr.write_record(&headers)?;

        // For each record, extract mapped fields and write a row
        for record in records {
            let mut row = vec!["".to_string(); headers.len()];

            // Map JSON fields to CSV columns
            for (json_path, csv_column) in &self.field_mappings {
                let pos = headers
                    .iter()
                    .position(|h| h == csv_column)
                    .ok_or_else(|| anyhow!("CSV column '{}' not found in headers", csv_column))?;

                // Extract value from JSON using simple path expression
                let value = extract_json_value(&record, json_path);
                if let Some(v) = value {
                    row[pos] = v;
                }
            }

            // Write row
            wtr.write_record(&row)?;
        }

        // Get CSV data
        let csv_data = wtr.into_inner()?;

        Ok(csv_data)
    }

    fn can_handle(&self, category: &DataCategory, data: &[u8]) -> bool {
        if !self.supported_categories.contains(category) {
            return false;
        }

        // Check if the data is valid JSON
        serde_json::from_slice::<JsonValue>(data).is_ok()
    }
}

/// Helper function to extract value from JSON using a simple path expression
fn extract_json_value(json: &JsonValue, path: &str) -> Option<String> {
    // Simple implementation of JSON path
    // Only supports basic paths like $.field.subfield

    let parts: Vec<&str> = path.trim_start_matches("$.").split('.').collect();

    let mut current = json;

    for part in &parts {
        if current.is_object() {
            current = &current[part];
        } else {
            return None;
        }

        if current.is_null() {
            return None;
        }
    }

    // Convert value to string
    if let Some(s) = current.as_str() {
        Some(s.to_string())
    } else if current.is_number() {
        Some(current.to_string())
    } else if let Some(b) = current.as_bool() {
        Some(b.to_string())
    } else {
        Some(current.to_string())
    }
}

/// JSON to XML adapter
pub struct JsonToXmlAdapter {
    /// Root element name
    root_element: String,
    /// Categories this adapter can handle
    supported_categories: Vec<DataCategory>,
}

impl JsonToXmlAdapter {
    /// Create a new JSON to XML adapter
    pub fn new(root_element: String) -> Self {
        JsonToXmlAdapter {
            root_element,
            supported_categories: vec![
                DataCategory::Weather,
                DataCategory::Seismic,
                DataCategory::AirQuality,
                DataCategory::Environmental,
            ],
        }
    }

    /// Add support for a category
    pub fn add_category(&mut self, category: DataCategory) {
        if !self.supported_categories.contains(&category) {
            self.supported_categories.push(category);
        }
    }
}

impl DataAdapter for JsonToXmlAdapter {
    fn source_format(&self) -> DataFormat {
        DataFormat::JSON
    }

    fn target_format(&self) -> DataFormat {
        DataFormat::XML
    }

    fn transform(&self, data: &[u8]) -> Result<Vec<u8>> {
        // Parse JSON data
        let json = serde_json::from_slice::<JsonValue>(data)
            .map_err(|e| anyhow!("Failed to parse JSON: {}", e))?;

        // Convert JSON to XML (simplified implementation)
        let mut xml = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<{}>",
            self.root_element
        );

        // If we have an array, wrap each item
        if let Some(array) = json.as_array() {
            for (i, item) in array.iter().enumerate() {
                xml.push_str(&format!("<item id=\"{}\">", i));
                xml.push_str(&json_value_to_xml(item, 1));
                xml.push_str("</item>");
            }
        } else {
            // Single object
            xml.push_str(&json_value_to_xml(&json, 0));
        }

        xml.push_str(&format!("</{}>", self.root_element));

        Ok(xml.into_bytes())
    }

    fn can_handle(&self, category: &DataCategory, data: &[u8]) -> bool {
        if !self.supported_categories.contains(category) {
            return false;
        }

        // Check if the data is valid JSON
        serde_json::from_slice::<JsonValue>(data).is_ok()
    }
}

/// Helper function to convert JSON value to XML string
fn json_value_to_xml(value: &JsonValue, indent_level: usize) -> String {
    let indent = "  ".repeat(indent_level);

    match value {
        JsonValue::Null => "".to_string(),
        JsonValue::Bool(b) => b.to_string(),
        JsonValue::Number(n) => n.to_string(),
        JsonValue::String(s) => escape_xml(s),
        JsonValue::Array(arr) => {
            let mut result = String::new();
            for (i, item) in arr.iter().enumerate() {
                result.push_str(&format!("\n{}  <item index=\"{}\">", indent, i));
                result.push_str(&json_value_to_xml(item, indent_level + 1));
                result.push_str(&format!("\n{}  </item>", indent));
            }
            result
        },
        JsonValue::Object(obj) => {
            let mut result = String::new();
            for (k, v) in obj {
                // Skip null values
                if v.is_null() {
                    continue;
                }

                let tag_name = sanitize_xml_tag(k);

                if v.is_object() || v.is_array() {
                    result.push_str(&format!("\n{}  <{}>", indent, tag_name));
                    result.push_str(&json_value_to_xml(v, indent_level + 1));
                    result.push_str(&format!("\n{}  </{}>", indent, tag_name));
                } else {
                    result.push_str(&format!(
                        "\n{}  <{}>{}</{}>",
                        indent,
                        tag_name,
                        json_value_to_xml(v, 0),
                        tag_name
                    ));
                }
            }
            result
        },
    }
}

/// Helper function to escape XML special characters
fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\'', "&apos;")
        .replace('"', "&quot;")
}

/// Helper function to sanitize XML tag names
fn sanitize_xml_tag(s: &str) -> String {
    // Replace invalid characters with underscores
    let mut result = String::new();
    for (i, c) in s.chars().enumerate() {
        if i == 0 {
            // First character must be a letter or underscore
            if c.is_alphabetic() || c == '_' {
                result.push(c);
            } else {
                result.push('_');
            }
        } else {
            // Subsequent characters can be letters, digits, dots, hyphens, or underscores
            if c.is_alphanumeric() || c == '.' || c == '-' || c == '_' {
                result.push(c);
            } else {
                result.push('_');
            }
        }
    }
    result
}

/// GeoJSON to standard JSON adapter
pub struct GeoJsonToJsonAdapter {
    /// Categories this adapter can handle
    supported_categories: Vec<DataCategory>,
}

impl GeoJsonToJsonAdapter {
    /// Create a new GeoJSON to JSON adapter
    pub fn new() -> Self {
        GeoJsonToJsonAdapter {
            supported_categories: vec![
                DataCategory::Seismic,
                DataCategory::Environmental,
                DataCategory::Humanitarian,
                DataCategory::Wildfire,
            ],
        }
    }

    /// Add support for a category
    pub fn add_category(&mut self, category: DataCategory) {
        if !self.supported_categories.contains(&category) {
            self.supported_categories.push(category);
        }
    }
}

impl DataAdapter for GeoJsonToJsonAdapter {
    fn source_format(&self) -> DataFormat {
        DataFormat::GeoJSON
    }

    fn target_format(&self) -> DataFormat {
        DataFormat::JSON
    }

    fn transform(&self, data: &[u8]) -> Result<Vec<u8>> {
        // Parse GeoJSON data
        let geojson = serde_json::from_slice::<JsonValue>(data)
            .map_err(|e| anyhow!("Failed to parse GeoJSON: {}", e))?;

        // GeoJSON should have a "type" field
        if !geojson.is_object() || !geojson["type"].is_string() {
            return Err(anyhow!("Invalid GeoJSON: missing or invalid 'type' field"));
        }

        // Transform GeoJSON to a simpler JSON structure
        let mut result = JsonValue::Object(serde_json::Map::new());

        // Extract type (guard above ensures this is a string)
        let geojson_type = geojson["type"]
            .as_str()
            .ok_or_else(|| anyhow!("GeoJSON 'type' field is not a string"))?;
        result["type"] = JsonValue::String(geojson_type.to_string());

        match geojson_type {
            "FeatureCollection" => {
                // Extract features
                if let Some(features) = geojson["features"].as_array() {
                    let mut simplified_features = Vec::new();

                    for feature in features {
                        let mut simple_feature = serde_json::Map::new();

                        // Copy properties
                        if let Some(properties) = feature["properties"].as_object() {
                            for (key, value) in properties {
                                simple_feature.insert(key.clone(), value.clone());
                            }
                        }

                        // Extract coordinates
                        if let Some(geometry) = feature["geometry"].as_object() {
                            if let Some(coordinates) = geometry["coordinates"].as_array() {
                                // For simplicity, we'll just extract the first coordinate
                                // In a real implementation, we would handle different geometry types
                                if !coordinates.is_empty() {
                                    simple_feature
                                        .insert("longitude".to_string(), coordinates[0].clone());
                                    if coordinates.len() > 1 {
                                        simple_feature
                                            .insert("latitude".to_string(), coordinates[1].clone());
                                    }
                                }
                            }
                        }

                        simplified_features.push(JsonValue::Object(simple_feature));
                    }

                    result["features"] = JsonValue::Array(simplified_features);
                }
            },
            "Feature" => {
                // Single feature - copy properties
                if let Some(properties) = geojson["properties"].as_object() {
                    for (key, value) in properties {
                        result[key] = value.clone();
                    }
                }

                // Extract coordinates
                if let Some(geometry) = geojson["geometry"].as_object() {
                    if let Some(coordinates) = geometry["coordinates"].as_array() {
                        if !coordinates.is_empty() {
                            result["longitude"] = coordinates[0].clone();
                            if coordinates.len() > 1 {
                                result["latitude"] = coordinates[1].clone();
                            }
                        }
                    }
                }
            },
            _ => {
                // Other types - just return the original
                return Ok(data.to_vec());
            },
        }

        // Convert to JSON
        let json_data = serde_json::to_vec(&result)?;

        Ok(json_data)
    }

    fn can_handle(&self, category: &DataCategory, data: &[u8]) -> bool {
        if !self.supported_categories.contains(category) {
            return false;
        }

        // Check if the data is valid GeoJSON
        if let Ok(json) = serde_json::from_slice::<JsonValue>(data) {
            if let Some(geojson_type) = json["type"].as_str() {
                return geojson_type == "FeatureCollection"
                    || geojson_type == "Feature"
                    || geojson_type == "Point"
                    || geojson_type == "LineString"
                    || geojson_type == "Polygon"
                    || geojson_type == "MultiPoint"
                    || geojson_type == "MultiLineString"
                    || geojson_type == "MultiPolygon"
                    || geojson_type == "GeometryCollection";
            }
        }

        false
    }
}

/// Factory function to create an appropriate adapter based on source and target formats
pub fn create_adapter(
    source_format: DataFormat,
    target_format: DataFormat,
) -> Result<Box<dyn DataAdapter>> {
    match (source_format, target_format) {
        (DataFormat::JSON, DataFormat::CSV) => Ok(Box::new(JsonToCsvAdapter::new())),
        (DataFormat::JSON, DataFormat::XML) => {
            Ok(Box::new(JsonToXmlAdapter::new("root".to_string())))
        },
        (DataFormat::GeoJSON, DataFormat::JSON) => Ok(Box::new(GeoJsonToJsonAdapter::new())),
        (source, target) => Err(anyhow!(
            "No adapter available for {} to {} conversion",
            source,
            target
        )),
    }
}

/// Adapter registry to manage available adapters
pub struct AdapterRegistry {
    /// Available adapters
    adapters: Vec<Box<dyn DataAdapter>>,
}

impl AdapterRegistry {
    /// Create a new adapter registry
    pub fn new() -> Self {
        AdapterRegistry {
            adapters: Vec::new(),
        }
    }

    /// Register an adapter
    pub fn register_adapter(&mut self, adapter: Box<dyn DataAdapter>) {
        self.adapters.push(adapter);
    }

    /// Find an adapter for the given formats and category
    pub fn find_adapter(
        &self,
        source_format: &DataFormat,
        target_format: &DataFormat,
        category: &DataCategory,
        data: &[u8],
    ) -> Option<&dyn DataAdapter> {
        for adapter in &self.adapters {
            if adapter.source_format() == *source_format
                && adapter.target_format() == *target_format
                && adapter.can_handle(category, data)
            {
                return Some(adapter.as_ref());
            }
        }

        None
    }

    /// Transform data using an appropriate adapter
    pub fn transform(
        &self,
        source_format: &DataFormat,
        target_format: &DataFormat,
        category: &DataCategory,
        data: &[u8],
    ) -> Result<Vec<u8>> {
        let adapter = self
            .find_adapter(source_format, target_format, category, data)
            .ok_or_else(|| {
                anyhow!(
                    "No suitable adapter found for {} to {} conversion for {:?}",
                    source_format,
                    target_format,
                    category
                )
            })?;

        adapter.transform(data)
    }

    /// Initialize with standard adapters
    pub fn initialize_standard_adapters() -> Self {
        let mut registry = AdapterRegistry::new();

        // JSON to CSV
        registry.register_adapter(Box::new(JsonToCsvAdapter::new()));

        // JSON to XML
        registry.register_adapter(Box::new(JsonToXmlAdapter::new("root".to_string())));

        // GeoJSON to JSON
        registry.register_adapter(Box::new(GeoJsonToJsonAdapter::new()));

        registry
    }
}
