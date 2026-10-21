//! Data schemas for standardizing and validating data from external sources
//!
//! This module provides definitions and validation logic for data schemas,
//! ensuring consistency and reliability across different data sources.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

/// Represents a data format
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DataFormat {
    /// JavaScript Object Notation
    JSON,
    /// Extensible Markup Language
    XML,
    /// Comma-Separated Values
    CSV,
    /// Network Common Data Form (scientific data)
    NetCDF,
    /// GeoJSON format for geographic data
    GeoJSON,
    /// Hierarchical Data Format (scientific data)
    HDF5,
    /// General Binary Data
    Binary,
    /// GRIB (GRIdded Binary) meteorological format
    GRIB,
    /// Google Protocol Buffers
    ProtoBuf,
    /// Custom format
    Custom(String),
}

impl fmt::Display for DataFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DataFormat::JSON => write!(f, "JSON"),
            DataFormat::XML => write!(f, "XML"),
            DataFormat::CSV => write!(f, "CSV"),
            DataFormat::NetCDF => write!(f, "NetCDF"),
            DataFormat::GeoJSON => write!(f, "GeoJSON"),
            DataFormat::HDF5 => write!(f, "HDF5"),
            DataFormat::Binary => write!(f, "Binary"),
            DataFormat::GRIB => write!(f, "GRIB"),
            DataFormat::ProtoBuf => write!(f, "ProtoBuf"),
            DataFormat::Custom(name) => write!(f, "Custom: {}", name),
        }
    }
}

/// Data schema field type
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FieldType {
    /// String value
    String,
    /// Integer value
    Integer,
    /// Floating point value
    Float,
    /// Boolean value
    Boolean,
    /// Date/time value
    DateTime,
    /// Array of values
    Array(Box<FieldType>),
    /// Nested object
    Object(HashMap<String, FieldDefinition>),
    /// Geographic coordinate
    GeoPoint,
    /// Geographic shape
    GeoShape,
    /// Binary data
    Binary,
    /// Enum of possible values
    Enum(Vec<String>),
}

/// Definition of a field in a schema
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldDefinition {
    /// Field name
    pub name: String,
    /// Field type
    pub field_type: FieldType,
    /// Field description
    pub description: String,
    /// Whether field is required
    pub required: bool,
    /// Default value (as JSON string)
    pub default_value: Option<String>,
    /// Validation rules
    pub validation: Option<ValidationRules>,
    /// Example value
    pub example: Option<String>,
    /// Units (e.g., "m/s", "°C")
    pub units: Option<String>,
}

/// Validation rules for a field
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValidationRules {
    /// Minimum value (numeric types)
    pub min_value: Option<f64>,
    /// Maximum value (numeric types)
    pub max_value: Option<f64>,
    /// Minimum length (strings, arrays)
    pub min_length: Option<usize>,
    /// Maximum length (strings, arrays)
    pub max_length: Option<usize>,
    /// Regular expression pattern (strings)
    pub pattern: Option<String>,
    /// Custom validation rules
    pub custom_rules: HashMap<String, String>,
}

/// Complete data schema definition
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DataSchema {
    /// Schema identifier
    pub id: String,
    /// Schema name
    pub name: String,
    /// Schema version
    pub version: String,
    /// Schema description
    pub description: String,
    /// Format this schema applies to
    pub format: DataFormat,
    /// Root fields
    pub fields: HashMap<String, FieldDefinition>,
    /// Schema documentation URL
    pub documentation_url: Option<String>,
    /// Example data (as string)
    pub example: Option<String>,
    /// Creator identifier
    pub creator: String,
    /// Creation date (ISO-8601)
    pub created_at: String,
    /// Last updated date (ISO-8601)
    pub updated_at: String,
    /// Parent schema (if extending another)
    pub parent_schema: Option<String>,
    /// Additional metadata
    pub metadata: HashMap<String, String>,
}

/// Trait for schema validators
pub trait SchemaValidator: Send + Sync {
    /// Validate data against this schema
    fn validate(&self, data: &[u8]) -> Result<bool>;

    /// Get validation errors (if any)
    fn get_validation_errors(&self) -> Vec<String>;
}

/// JSON Schema validator
pub struct JsonSchemaValidator {
    /// Schema ID
    pub schema_id: String,
    /// JSON Schema as string
    pub schema_json: String,
    /// Last validation errors
    validation_errors: Vec<String>,
}

impl JsonSchemaValidator {
    /// Create a new JSON schema validator
    pub fn new(schema_id: String, schema_json: String) -> Self {
        JsonSchemaValidator {
            schema_id,
            schema_json,
            validation_errors: Vec::new(),
        }
    }
}

impl SchemaValidator for JsonSchemaValidator {
    fn validate(&self, _data: &[u8]) -> Result<bool> {
        // In a real implementation, this would use a JSON Schema validator
        // like jsonschema or valico to validate the data against the schema

        // For now, we'll just return true
        Ok(true)
    }

    fn get_validation_errors(&self) -> Vec<String> {
        self.validation_errors.clone()
    }
}

/// XML Schema validator
pub struct XmlSchemaValidator {
    /// Schema ID
    pub schema_id: String,
    /// XSD Schema as string
    pub xsd_schema: String,
    /// Last validation errors
    validation_errors: Vec<String>,
}

impl XmlSchemaValidator {
    /// Create a new XML schema validator
    pub fn new(schema_id: String, xsd_schema: String) -> Self {
        XmlSchemaValidator {
            schema_id,
            xsd_schema,
            validation_errors: Vec::new(),
        }
    }
}

impl SchemaValidator for XmlSchemaValidator {
    fn validate(&self, _data: &[u8]) -> Result<bool> {
        // In a real implementation, this would use an XML Schema validator
        // like xml-rs with an XSD validator to check the data

        // For now, we'll just return true
        Ok(true)
    }

    fn get_validation_errors(&self) -> Vec<String> {
        self.validation_errors.clone()
    }
}

/// CSV Schema validator
pub struct CsvSchemaValidator {
    /// Schema ID
    pub schema_id: String,
    /// Field definitions
    pub fields: Vec<FieldDefinition>,
    /// Has header row
    pub has_header: bool,
    /// Delimiter character
    pub delimiter: char,
    /// Last validation errors
    validation_errors: Vec<String>,
}

impl CsvSchemaValidator {
    /// Create a new CSV schema validator
    pub fn new(
        schema_id: String,
        fields: Vec<FieldDefinition>,
        has_header: bool,
        delimiter: char,
    ) -> Self {
        CsvSchemaValidator {
            schema_id,
            fields,
            has_header,
            delimiter,
            validation_errors: Vec::new(),
        }
    }
}

impl SchemaValidator for CsvSchemaValidator {
    fn validate(&self, _data: &[u8]) -> Result<bool> {
        // In a real implementation, this would parse the CSV data
        // and validate it against the field definitions

        // For now, we'll just return true
        Ok(true)
    }

    fn get_validation_errors(&self) -> Vec<String> {
        self.validation_errors.clone()
    }
}

/// Binary format schema validator
pub struct BinarySchemaValidator {
    /// Schema ID
    pub schema_id: String,
    /// Field definitions and byte offsets
    pub field_layouts: Vec<(usize, usize, FieldDefinition)>,
    /// Endianness (big or little)
    pub big_endian: bool,
    /// Last validation errors
    validation_errors: Vec<String>,
}

impl BinarySchemaValidator {
    /// Create a new binary schema validator
    pub fn new(
        schema_id: String,
        field_layouts: Vec<(usize, usize, FieldDefinition)>,
        big_endian: bool,
    ) -> Self {
        BinarySchemaValidator {
            schema_id,
            field_layouts,
            big_endian,
            validation_errors: Vec::new(),
        }
    }
}

impl SchemaValidator for BinarySchemaValidator {
    fn validate(&self, _data: &[u8]) -> Result<bool> {
        // In a real implementation, this would interpret the binary data
        // according to the field layouts and validate each field

        // For now, we'll just return true
        Ok(true)
    }

    fn get_validation_errors(&self) -> Vec<String> {
        self.validation_errors.clone()
    }
}

/// Create a standard weather data schema
pub fn create_weather_schema() -> DataSchema {
    let mut fields = HashMap::new();

    // Time field
    fields.insert(
        "timestamp".to_string(),
        FieldDefinition {
            name: "timestamp".to_string(),
            field_type: FieldType::DateTime,
            description: "Observation time".to_string(),
            required: true,
            default_value: None,
            validation: None,
            example: Some("2023-05-15T12:30:00Z".to_string()),
            units: None,
        },
    );

    // Location fields
    fields.insert(
        "location".to_string(),
        FieldDefinition {
            name: "location".to_string(),
            field_type: FieldType::GeoPoint,
            description: "Observation location".to_string(),
            required: true,
            default_value: None,
            validation: None,
            example: Some("[47.6062, -122.3321]".to_string()),
            units: None,
        },
    );

    // Temperature field
    fields.insert(
        "temperature".to_string(),
        FieldDefinition {
            name: "temperature".to_string(),
            field_type: FieldType::Float,
            description: "Air temperature".to_string(),
            required: true,
            default_value: None,
            validation: Some(ValidationRules {
                min_value: Some(-100.0),
                max_value: Some(100.0),
                min_length: None,
                max_length: None,
                pattern: None,
                custom_rules: HashMap::new(),
            }),
            example: Some("22.5".to_string()),
            units: Some("°C".to_string()),
        },
    );

    // Humidity field
    fields.insert(
        "humidity".to_string(),
        FieldDefinition {
            name: "humidity".to_string(),
            field_type: FieldType::Float,
            description: "Relative humidity".to_string(),
            required: true,
            default_value: None,
            validation: Some(ValidationRules {
                min_value: Some(0.0),
                max_value: Some(100.0),
                min_length: None,
                max_length: None,
                pattern: None,
                custom_rules: HashMap::new(),
            }),
            example: Some("65.0".to_string()),
            units: Some("%".to_string()),
        },
    );

    // Wind fields
    let mut wind_fields = HashMap::new();

    wind_fields.insert(
        "speed".to_string(),
        FieldDefinition {
            name: "speed".to_string(),
            field_type: FieldType::Float,
            description: "Wind speed".to_string(),
            required: true,
            default_value: None,
            validation: Some(ValidationRules {
                min_value: Some(0.0),
                max_value: None,
                min_length: None,
                max_length: None,
                pattern: None,
                custom_rules: HashMap::new(),
            }),
            example: Some("15.2".to_string()),
            units: Some("m/s".to_string()),
        },
    );

    wind_fields.insert(
        "direction".to_string(),
        FieldDefinition {
            name: "direction".to_string(),
            field_type: FieldType::Float,
            description: "Wind direction".to_string(),
            required: true,
            default_value: None,
            validation: Some(ValidationRules {
                min_value: Some(0.0),
                max_value: Some(360.0),
                min_length: None,
                max_length: None,
                pattern: None,
                custom_rules: HashMap::new(),
            }),
            example: Some("270.0".to_string()),
            units: Some("degrees".to_string()),
        },
    );

    wind_fields.insert(
        "gust".to_string(),
        FieldDefinition {
            name: "gust".to_string(),
            field_type: FieldType::Float,
            description: "Wind gust speed".to_string(),
            required: false,
            default_value: None,
            validation: Some(ValidationRules {
                min_value: Some(0.0),
                max_value: None,
                min_length: None,
                max_length: None,
                pattern: None,
                custom_rules: HashMap::new(),
            }),
            example: Some("25.7".to_string()),
            units: Some("m/s".to_string()),
        },
    );

    fields.insert(
        "wind".to_string(),
        FieldDefinition {
            name: "wind".to_string(),
            field_type: FieldType::Object(wind_fields),
            description: "Wind measurements".to_string(),
            required: true,
            default_value: None,
            validation: None,
            example: None,
            units: None,
        },
    );

    // Pressure field
    fields.insert(
        "pressure".to_string(),
        FieldDefinition {
            name: "pressure".to_string(),
            field_type: FieldType::Float,
            description: "Atmospheric pressure".to_string(),
            required: true,
            default_value: None,
            validation: Some(ValidationRules {
                min_value: Some(800.0),
                max_value: Some(1200.0),
                min_length: None,
                max_length: None,
                pattern: None,
                custom_rules: HashMap::new(),
            }),
            example: Some("1013.25".to_string()),
            units: Some("hPa".to_string()),
        },
    );

    // Precipitation field
    fields.insert(
        "precipitation".to_string(),
        FieldDefinition {
            name: "precipitation".to_string(),
            field_type: FieldType::Float,
            description: "Precipitation amount".to_string(),
            required: false,
            default_value: Some("0.0".to_string()),
            validation: Some(ValidationRules {
                min_value: Some(0.0),
                max_value: None,
                min_length: None,
                max_length: None,
                pattern: None,
                custom_rules: HashMap::new(),
            }),
            example: Some("2.5".to_string()),
            units: Some("mm".to_string()),
        },
    );

    // Cloud cover field
    fields.insert(
        "cloud_cover".to_string(),
        FieldDefinition {
            name: "cloud_cover".to_string(),
            field_type: FieldType::Float,
            description: "Cloud cover percentage".to_string(),
            required: false,
            default_value: None,
            validation: Some(ValidationRules {
                min_value: Some(0.0),
                max_value: Some(100.0),
                min_length: None,
                max_length: None,
                pattern: None,
                custom_rules: HashMap::new(),
            }),
            example: Some("75.0".to_string()),
            units: Some("%".to_string()),
        },
    );

    // Visibility field
    fields.insert(
        "visibility".to_string(),
        FieldDefinition {
            name: "visibility".to_string(),
            field_type: FieldType::Float,
            description: "Visibility distance".to_string(),
            required: false,
            default_value: None,
            validation: Some(ValidationRules {
                min_value: Some(0.0),
                max_value: None,
                min_length: None,
                max_length: None,
                pattern: None,
                custom_rules: HashMap::new(),
            }),
            example: Some("10000.0".to_string()),
            units: Some("m".to_string()),
        },
    );

    // Weather condition field
    fields.insert(
        "condition".to_string(),
        FieldDefinition {
            name: "condition".to_string(),
            field_type: FieldType::Enum(vec![
                "clear".to_string(),
                "partly_cloudy".to_string(),
                "cloudy".to_string(),
                "rain".to_string(),
                "snow".to_string(),
                "sleet".to_string(),
                "fog".to_string(),
                "thunderstorm".to_string(),
                "windy".to_string(),
                "hail".to_string(),
                "tornado".to_string(),
                "hurricane".to_string(),
            ]),
            description: "Weather condition".to_string(),
            required: false,
            default_value: None,
            validation: None,
            example: Some("partly_cloudy".to_string()),
            units: None,
        },
    );

    // Source field
    fields.insert(
        "source".to_string(),
        FieldDefinition {
            name: "source".to_string(),
            field_type: FieldType::String,
            description: "Data source identifier".to_string(),
            required: true,
            default_value: None,
            validation: None,
            example: Some("NOAA".to_string()),
            units: None,
        },
    );

    // Station field
    fields.insert(
        "station_id".to_string(),
        FieldDefinition {
            name: "station_id".to_string(),
            field_type: FieldType::String,
            description: "Weather station identifier".to_string(),
            required: false,
            default_value: None,
            validation: None,
            example: Some("KNYC".to_string()),
            units: None,
        },
    );

    // Build the schema
    DataSchema {
        id: "weather-observation-v1".to_string(),
        name: "Weather Observation".to_string(),
        version: "1.0".to_string(),
        description: "Standard schema for weather observations from various sources".to_string(),
        format: DataFormat::JSON,
        fields,
        documentation_url: Some("https://wrfcoin.io/schemas/weather-observation-v1".to_string()),
        example: None,
        creator: "wrfcoin".to_string(),
        created_at: "2023-01-01T00:00:00Z".to_string(),
        updated_at: "2023-01-01T00:00:00Z".to_string(),
        parent_schema: None,
        metadata: HashMap::new(),
    }
}

/// Create a standard seismic data schema
pub fn create_seismic_schema() -> DataSchema {
    let mut fields = HashMap::new();

    // Event time field
    fields.insert(
        "timestamp".to_string(),
        FieldDefinition {
            name: "timestamp".to_string(),
            field_type: FieldType::DateTime,
            description: "Event time".to_string(),
            required: true,
            default_value: None,
            validation: None,
            example: Some("2023-05-15T12:30:00Z".to_string()),
            units: None,
        },
    );

    // Event location fields
    fields.insert(
        "location".to_string(),
        FieldDefinition {
            name: "location".to_string(),
            field_type: FieldType::GeoPoint,
            description: "Event epicenter location".to_string(),
            required: true,
            default_value: None,
            validation: None,
            example: Some("[37.7749, -122.4194]".to_string()),
            units: None,
        },
    );

    // Depth field
    fields.insert(
        "depth".to_string(),
        FieldDefinition {
            name: "depth".to_string(),
            field_type: FieldType::Float,
            description: "Depth of the earthquake focus".to_string(),
            required: true,
            default_value: None,
            validation: Some(ValidationRules {
                min_value: Some(0.0),
                max_value: Some(800.0),
                min_length: None,
                max_length: None,
                pattern: None,
                custom_rules: HashMap::new(),
            }),
            example: Some("10.5".to_string()),
            units: Some("km".to_string()),
        },
    );

    // Magnitude field
    fields.insert(
        "magnitude".to_string(),
        FieldDefinition {
            name: "magnitude".to_string(),
            field_type: FieldType::Float,
            description: "Earthquake magnitude".to_string(),
            required: true,
            default_value: None,
            validation: Some(ValidationRules {
                min_value: Some(0.0),
                max_value: Some(10.0),
                min_length: None,
                max_length: None,
                pattern: None,
                custom_rules: HashMap::new(),
            }),
            example: Some("4.5".to_string()),
            units: None,
        },
    );

    // Magnitude type field
    fields.insert(
        "magnitude_type".to_string(),
        FieldDefinition {
            name: "magnitude_type".to_string(),
            field_type: FieldType::Enum(vec![
                "ml".to_string(), // Local magnitude
                "mb".to_string(), // Body-wave magnitude
                "ms".to_string(), // Surface-wave magnitude
                "mw".to_string(), // Moment magnitude
                "md".to_string(), // Duration magnitude
            ]),
            description: "Type of magnitude measurement".to_string(),
            required: true,
            default_value: None,
            validation: None,
            example: Some("mw".to_string()),
            units: None,
        },
    );

    // Event ID field
    fields.insert(
        "event_id".to_string(),
        FieldDefinition {
            name: "event_id".to_string(),
            field_type: FieldType::String,
            description: "Unique identifier for the seismic event".to_string(),
            required: true,
            default_value: None,
            validation: None,
            example: Some("us2023abcd".to_string()),
            units: None,
        },
    );

    // Felt reports field
    fields.insert(
        "felt_reports".to_string(),
        FieldDefinition {
            name: "felt_reports".to_string(),
            field_type: FieldType::Integer,
            description: "Number of felt reports submitted by the public".to_string(),
            required: false,
            default_value: Some("0".to_string()),
            validation: Some(ValidationRules {
                min_value: Some(0.0),
                max_value: None,
                min_length: None,
                max_length: None,
                pattern: None,
                custom_rules: HashMap::new(),
            }),
            example: Some("157".to_string()),
            units: None,
        },
    );

    // Maximum intensity field
    fields.insert(
        "max_intensity".to_string(),
        FieldDefinition {
            name: "max_intensity".to_string(),
            field_type: FieldType::Float,
            description: "Maximum seismic intensity (MMI scale)".to_string(),
            required: false,
            default_value: None,
            validation: Some(ValidationRules {
                min_value: Some(0.0),
                max_value: Some(12.0),
                min_length: None,
                max_length: None,
                pattern: None,
                custom_rules: HashMap::new(),
            }),
            example: Some("4.5".to_string()),
            units: None,
        },
    );

    // Tsunami warning field
    fields.insert(
        "tsunami_warning".to_string(),
        FieldDefinition {
            name: "tsunami_warning".to_string(),
            field_type: FieldType::Boolean,
            description: "Whether a tsunami warning was issued".to_string(),
            required: false,
            default_value: Some("false".to_string()),
            validation: None,
            example: Some("false".to_string()),
            units: None,
        },
    );

    // Source field
    fields.insert(
        "source".to_string(),
        FieldDefinition {
            name: "source".to_string(),
            field_type: FieldType::String,
            description: "Data source identifier".to_string(),
            required: true,
            default_value: None,
            validation: None,
            example: Some("USGS".to_string()),
            units: None,
        },
    );

    // Alert level field
    fields.insert(
        "alert_level".to_string(),
        FieldDefinition {
            name: "alert_level".to_string(),
            field_type: FieldType::Enum(vec![
                "green".to_string(),
                "yellow".to_string(),
                "orange".to_string(),
                "red".to_string(),
            ]),
            description: "Alert level based on magnitude and potential impact".to_string(),
            required: false,
            default_value: None,
            validation: None,
            example: Some("yellow".to_string()),
            units: None,
        },
    );

    // Build the schema
    DataSchema {
        id: "seismic-event-v1".to_string(),
        name: "Seismic Event".to_string(),
        version: "1.0".to_string(),
        description: "Standard schema for seismic events from various sources".to_string(),
        format: DataFormat::JSON,
        fields,
        documentation_url: Some("https://wrfcoin.io/schemas/seismic-event-v1".to_string()),
        example: None,
        creator: "wrfcoin".to_string(),
        created_at: "2023-01-01T00:00:00Z".to_string(),
        updated_at: "2023-01-01T00:00:00Z".to_string(),
        parent_schema: None,
        metadata: HashMap::new(),
    }
}

/// Create a standard air quality data schema
pub fn create_air_quality_schema() -> DataSchema {
    let mut fields = HashMap::new();

    // Time field
    fields.insert(
        "timestamp".to_string(),
        FieldDefinition {
            name: "timestamp".to_string(),
            field_type: FieldType::DateTime,
            description: "Measurement time".to_string(),
            required: true,
            default_value: None,
            validation: None,
            example: Some("2023-05-15T12:30:00Z".to_string()),
            units: None,
        },
    );

    // Location fields
    fields.insert(
        "location".to_string(),
        FieldDefinition {
            name: "location".to_string(),
            field_type: FieldType::GeoPoint,
            description: "Measurement location".to_string(),
            required: true,
            default_value: None,
            validation: None,
            example: Some("[34.0522, -118.2437]".to_string()),
            units: None,
        },
    );

    // AQI field
    fields.insert(
        "aqi".to_string(),
        FieldDefinition {
            name: "aqi".to_string(),
            field_type: FieldType::Integer,
            description: "Air Quality Index value".to_string(),
            required: true,
            default_value: None,
            validation: Some(ValidationRules {
                min_value: Some(0.0),
                max_value: Some(500.0),
                min_length: None,
                max_length: None,
                pattern: None,
                custom_rules: HashMap::new(),
            }),
            example: Some("56".to_string()),
            units: None,
        },
    );

    // PM2.5 field
    fields.insert(
        "pm25".to_string(),
        FieldDefinition {
            name: "pm25".to_string(),
            field_type: FieldType::Float,
            description: "PM2.5 concentration".to_string(),
            required: false,
            default_value: None,
            validation: Some(ValidationRules {
                min_value: Some(0.0),
                max_value: None,
                min_length: None,
                max_length: None,
                pattern: None,
                custom_rules: HashMap::new(),
            }),
            example: Some("12.5".to_string()),
            units: Some("µg/m³".to_string()),
        },
    );

    // PM10 field
    fields.insert(
        "pm10".to_string(),
        FieldDefinition {
            name: "pm10".to_string(),
            field_type: FieldType::Float,
            description: "PM10 concentration".to_string(),
            required: false,
            default_value: None,
            validation: Some(ValidationRules {
                min_value: Some(0.0),
                max_value: None,
                min_length: None,
                max_length: None,
                pattern: None,
                custom_rules: HashMap::new(),
            }),
            example: Some("25.3".to_string()),
            units: Some("µg/m³".to_string()),
        },
    );

    // O3 (Ozone) field
    fields.insert(
        "o3".to_string(),
        FieldDefinition {
            name: "o3".to_string(),
            field_type: FieldType::Float,
            description: "Ozone concentration".to_string(),
            required: false,
            default_value: None,
            validation: Some(ValidationRules {
                min_value: Some(0.0),
                max_value: None,
                min_length: None,
                max_length: None,
                pattern: None,
                custom_rules: HashMap::new(),
            }),
            example: Some("0.035".to_string()),
            units: Some("ppm".to_string()),
        },
    );

    // NO2 (Nitrogen Dioxide) field
    fields.insert(
        "no2".to_string(),
        FieldDefinition {
            name: "no2".to_string(),
            field_type: FieldType::Float,
            description: "Nitrogen dioxide concentration".to_string(),
            required: false,
            default_value: None,
            validation: Some(ValidationRules {
                min_value: Some(0.0),
                max_value: None,
                min_length: None,
                max_length: None,
                pattern: None,
                custom_rules: HashMap::new(),
            }),
            example: Some("0.012".to_string()),
            units: Some("ppm".to_string()),
        },
    );

    // SO2 (Sulfur Dioxide) field
    fields.insert(
        "so2".to_string(),
        FieldDefinition {
            name: "so2".to_string(),
            field_type: FieldType::Float,
            description: "Sulfur dioxide concentration".to_string(),
            required: false,
            default_value: None,
            validation: Some(ValidationRules {
                min_value: Some(0.0),
                max_value: None,
                min_length: None,
                max_length: None,
                pattern: None,
                custom_rules: HashMap::new(),
            }),
            example: Some("0.003".to_string()),
            units: Some("ppm".to_string()),
        },
    );

    // CO (Carbon Monoxide) field
    fields.insert(
        "co".to_string(),
        FieldDefinition {
            name: "co".to_string(),
            field_type: FieldType::Float,
            description: "Carbon monoxide concentration".to_string(),
            required: false,
            default_value: None,
            validation: Some(ValidationRules {
                min_value: Some(0.0),
                max_value: None,
                min_length: None,
                max_length: None,
                pattern: None,
                custom_rules: HashMap::new(),
            }),
            example: Some("0.5".to_string()),
            units: Some("ppm".to_string()),
        },
    );

    // Air quality category field
    fields.insert(
        "category".to_string(),
        FieldDefinition {
            name: "category".to_string(),
            field_type: FieldType::Enum(vec![
                "good".to_string(),
                "moderate".to_string(),
                "unhealthy_for_sensitive_groups".to_string(),
                "unhealthy".to_string(),
                "very_unhealthy".to_string(),
                "hazardous".to_string(),
            ]),
            description: "Air quality category based on AQI".to_string(),
            required: false,
            default_value: None,
            validation: None,
            example: Some("moderate".to_string()),
            units: None,
        },
    );

    // Station field
    fields.insert(
        "station_id".to_string(),
        FieldDefinition {
            name: "station_id".to_string(),
            field_type: FieldType::String,
            description: "Air quality monitoring station identifier".to_string(),
            required: false,
            default_value: None,
            validation: None,
            example: Some("LACSD".to_string()),
            units: None,
        },
    );

    // Source field
    fields.insert(
        "source".to_string(),
        FieldDefinition {
            name: "source".to_string(),
            field_type: FieldType::String,
            description: "Data source identifier".to_string(),
            required: true,
            default_value: None,
            validation: None,
            example: Some("EPA".to_string()),
            units: None,
        },
    );

    // Build the schema
    DataSchema {
        id: "air-quality-observation-v1".to_string(),
        name: "Air Quality Observation".to_string(),
        version: "1.0".to_string(),
        description: "Standard schema for air quality observations from various sources"
            .to_string(),
        format: DataFormat::JSON,
        fields,
        documentation_url: Some(
            "https://wrfcoin.io/schemas/air-quality-observation-v1".to_string(),
        ),
        example: None,
        creator: "wrfcoin".to_string(),
        created_at: "2023-01-01T00:00:00Z".to_string(),
        updated_at: "2023-01-01T00:00:00Z".to_string(),
        parent_schema: None,
        metadata: HashMap::new(),
    }
}

/// Create a standard acoustic gunshot detection schema
pub fn create_gunshot_detection_schema() -> DataSchema {
    let mut fields = HashMap::new();

    // Event time field
    fields.insert(
        "timestamp".to_string(),
        FieldDefinition {
            name: "timestamp".to_string(),
            field_type: FieldType::DateTime,
            description: "Detection time".to_string(),
            required: true,
            default_value: None,
            validation: None,
            example: Some("2023-05-15T22:30:45Z".to_string()),
            units: None,
        },
    );

    // Location fields
    fields.insert(
        "location".to_string(),
        FieldDefinition {
            name: "location".to_string(),
            field_type: FieldType::GeoPoint,
            description: "Detection location".to_string(),
            required: true,
            default_value: None,
            validation: None,
            example: Some("[41.8781, -87.6298]".to_string()),
            units: None,
        },
    );

    // Detection ID field
    fields.insert(
        "detection_id".to_string(),
        FieldDefinition {
            name: "detection_id".to_string(),
            field_type: FieldType::String,
            description: "Unique identifier for the acoustic detection".to_string(),
            required: true,
            default_value: None,
            validation: None,
            example: Some("GS-2023051522304501".to_string()),
            units: None,
        },
    );

    // Confidence score field
    fields.insert(
        "confidence".to_string(),
        FieldDefinition {
            name: "confidence".to_string(),
            field_type: FieldType::Float,
            description: "Confidence score of the detection".to_string(),
            required: true,
            default_value: None,
            validation: Some(ValidationRules {
                min_value: Some(0.0),
                max_value: Some(100.0),
                min_length: None,
                max_length: None,
                pattern: None,
                custom_rules: HashMap::new(),
            }),
            example: Some("87.5".to_string()),
            units: Some("%".to_string()),
        },
    );

    // Type field
    fields.insert(
        "type".to_string(),
        FieldDefinition {
            name: "type".to_string(),
            field_type: FieldType::Enum(vec![
                "gunshot".to_string(),
                "possible_gunshot".to_string(),
                "multiple_gunshots".to_string(),
                "firecracker".to_string(),
                "other_explosion".to_string(),
            ]),
            description: "Type of acoustic event detected".to_string(),
            required: true,
            default_value: None,
            validation: None,
            example: Some("gunshot".to_string()),
            units: None,
        },
    );

    // Round count field
    fields.insert(
        "round_count".to_string(),
        FieldDefinition {
            name: "round_count".to_string(),
            field_type: FieldType::Integer,
            description: "Estimated number of rounds fired".to_string(),
            required: false,
            default_value: Some("1".to_string()),
            validation: Some(ValidationRules {
                min_value: Some(1.0),
                max_value: None,
                min_length: None,
                max_length: None,
                pattern: None,
                custom_rules: HashMap::new(),
            }),
            example: Some("3".to_string()),
            units: None,
        },
    );

    // Sensor count field
    fields.insert(
        "sensor_count".to_string(),
        FieldDefinition {
            name: "sensor_count".to_string(),
            field_type: FieldType::Integer,
            description: "Number of sensors that detected the event".to_string(),
            required: true,
            default_value: None,
            validation: Some(ValidationRules {
                min_value: Some(1.0),
                max_value: None,
                min_length: None,
                max_length: None,
                pattern: None,
                custom_rules: HashMap::new(),
            }),
            example: Some("4".to_string()),
            units: None,
        },
    );

    // Location accuracy field
    fields.insert(
        "location_accuracy".to_string(),
        FieldDefinition {
            name: "location_accuracy".to_string(),
            field_type: FieldType::Float,
            description: "Accuracy radius of the detection location".to_string(),
            required: false,
            default_value: None,
            validation: Some(ValidationRules {
                min_value: Some(0.0),
                max_value: None,
                min_length: None,
                max_length: None,
                pattern: None,
                custom_rules: HashMap::new(),
            }),
            example: Some("25.0".to_string()),
            units: Some("m".to_string()),
        },
    );

    // Response status field
    fields.insert(
        "response_status".to_string(),
        FieldDefinition {
            name: "response_status".to_string(),
            field_type: FieldType::Enum(vec![
                "pending".to_string(),
                "dispatched".to_string(),
                "responding".to_string(),
                "on_scene".to_string(),
                "resolved".to_string(),
                "false_alarm".to_string(),
            ]),
            description: "Status of emergency response to the event".to_string(),
            required: false,
            default_value: Some("pending".to_string()),
            validation: None,
            example: Some("dispatched".to_string()),
            units: None,
        },
    );

    // Audio clip field (optional)
    fields.insert(
        "audio_clip".to_string(),
        FieldDefinition {
            name: "audio_clip".to_string(),
            field_type: FieldType::Binary,
            description: "Brief audio clip of the detected sound (if available)".to_string(),
            required: false,
            default_value: None,
            validation: None,
            example: None,
            units: None,
        },
    );

    // Source field
    fields.insert(
        "source".to_string(),
        FieldDefinition {
            name: "source".to_string(),
            field_type: FieldType::String,
            description: "Detection system identifier".to_string(),
            required: true,
            default_value: None,
            validation: None,
            example: Some("ShotSpotter".to_string()),
            units: None,
        },
    );

    // Build the schema
    DataSchema {
        id: "gunshot-detection-v1".to_string(),
        name: "Gunshot Detection".to_string(),
        version: "1.0".to_string(),
        description: "Standard schema for acoustic gunshot detections".to_string(),
        format: DataFormat::JSON,
        fields,
        documentation_url: Some("https://wrfcoin.io/schemas/gunshot-detection-v1".to_string()),
        example: None,
        creator: "wrfcoin".to_string(),
        created_at: "2023-01-01T00:00:00Z".to_string(),
        updated_at: "2023-01-01T00:00:00Z".to_string(),
        parent_schema: None,
        metadata: HashMap::new(),
    }
}

// Additional schemas would be defined similarly
