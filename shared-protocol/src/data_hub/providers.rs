//! Data providers module
//!
//! This module defines official data providers and their sources that
//! are integrated with the Wrfcoin network.

use super::{DataCategory, DataFormat, DataProvider, DataSource};
use chrono::{DateTime, Utc};
use std::collections::HashMap;

/// Create a provider for NOAA (National Oceanic and Atmospheric Administration)
pub fn create_noaa_provider() -> DataProvider {
    DataProvider {
        id: "noaa".to_string(),
        name: "National Oceanic and Atmospheric Administration".to_string(),
        description: "U.S. agency focused on the condition of the oceans and atmosphere."
            .to_string(),
        website: "https://www.noaa.gov/".to_string(),
        contact: "contact@noaa.gov".to_string(),
        data_sources: vec![
            "noaa-nws".to_string(),
            "noaa-ncei".to_string(),
            "noaa-ndbc".to_string(),
        ],
        is_verified: true,
        registered_at: Utc::now(),
        terms_url: Some("https://www.noaa.gov/terms-of-use".to_string()),
        metadata: HashMap::new(),
    }
}

/// Create a provider for USGS (United States Geological Survey)
pub fn create_usgs_provider() -> DataProvider {
    DataProvider {
        id: "usgs".to_string(),
        name: "United States Geological Survey".to_string(),
        description: "U.S. government agency that studies the landscape, natural resources, and natural hazards.".to_string(),
        website: "https://www.usgs.gov/".to_string(),
        contact: "contact@usgs.gov".to_string(),
        data_sources: vec![
            "usgs-earthquake".to_string(),
            "usgs-waterdata".to_string(),
            "usgs-landsat".to_string(),
        ],
        is_verified: true,
        registered_at: Utc::now(),
        terms_url: Some("https://www.usgs.gov/information-policies-and-instructions/copyrights-and-credits".to_string()),
        metadata: HashMap::new(),
    }
}

/// Create a provider for EPA (Environmental Protection Agency)
pub fn create_epa_provider() -> DataProvider {
    DataProvider {
        id: "epa".to_string(),
        name: "Environmental Protection Agency".to_string(),
        description: "U.S. agency for protecting human health and the environment.".to_string(),
        website: "https://www.epa.gov/".to_string(),
        contact: "contact@epa.gov".to_string(),
        data_sources: vec![
            "epa-airnow".to_string(),
            "epa-waterquality".to_string(),
            "epa-envirofacts".to_string(),
        ],
        is_verified: true,
        registered_at: Utc::now(),
        terms_url: Some("https://www.epa.gov/privacy/privacy-and-security-notice".to_string()),
        metadata: HashMap::new(),
    }
}

/// Create a provider for NASA (National Aeronautics and Space Administration)
pub fn create_nasa_provider() -> DataProvider {
    DataProvider {
        id: "nasa".to_string(),
        name: "National Aeronautics and Space Administration".to_string(),
        description:
            "U.S. agency responsible for space program, aeronautics and aerospace research."
                .to_string(),
        website: "https://www.nasa.gov/".to_string(),
        contact: "contact@nasa.gov".to_string(),
        data_sources: vec![
            "nasa-power".to_string(),
            "nasa-earthdata".to_string(),
            "nasa-gpm".to_string(),
            "nasa-firms".to_string(),
        ],
        is_verified: true,
        registered_at: Utc::now(),
        terms_url: Some("https://www.nasa.gov/about/highlights/HP_Privacy.html".to_string()),
        metadata: HashMap::new(),
    }
}

/// Create a provider for ESA (European Space Agency)
pub fn create_esa_provider() -> DataProvider {
    DataProvider {
        id: "esa".to_string(),
        name: "European Space Agency".to_string(),
        description:
            "Europe's gateway to space, dedicated to space exploration and Earth observation."
                .to_string(),
        website: "https://www.esa.int/".to_string(),
        contact: "contact@esa.int".to_string(),
        data_sources: vec![
            "esa-sentinel".to_string(),
            "esa-cams".to_string(),
            "esa-cds".to_string(),
        ],
        is_verified: true,
        registered_at: Utc::now(),
        terms_url: Some("https://www.esa.int/Services/Terms_and_conditions".to_string()),
        metadata: HashMap::new(),
    }
}

/// Create a provider for EMSC (European-Mediterranean Seismological Centre)
pub fn create_emsc_provider() -> DataProvider {
    DataProvider {
        id: "emsc".to_string(),
        name: "European-Mediterranean Seismological Centre".to_string(),
        description: "International, non-governmental, scientific organization providing earthquake information.".to_string(),
        website: "https://www.emsc-csem.org/".to_string(),
        contact: "contact@emsc-csem.org".to_string(),
        data_sources: vec![
            "emsc-earthquake".to_string(),
        ],
        is_verified: true,
        registered_at: Utc::now(),
        terms_url: Some("https://www.emsc-csem.org/service/application/webservices.php".to_string()),
        metadata: HashMap::new(),
    }
}

/// Create a provider for WHO (World Health Organization)
pub fn create_who_provider() -> DataProvider {
    DataProvider {
        id: "who".to_string(),
        name: "World Health Organization".to_string(),
        description: "United Nations agency that connects nations, partners and people to promote health and serve the vulnerable.".to_string(),
        website: "https://www.who.int/".to_string(),
        contact: "contact@who.int".to_string(),
        data_sources: vec![
            "who-diseases".to_string(),
            "who-air-quality".to_string(),
            "who-healthstatus".to_string(),
        ],
        is_verified: true,
        registered_at: Utc::now(),
        terms_url: Some("https://www.who.int/about/policies/publishing/copyright".to_string()),
        metadata: HashMap::new(),
    }
}

/// Create a provider for UNHCR (United Nations High Commissioner for Refugees)
pub fn create_unhcr_provider() -> DataProvider {
    DataProvider {
        id: "unhcr".to_string(),
        name: "United Nations High Commissioner for Refugees".to_string(),
        description: "UN agency mandated to aid and protect refugees, forcibly displaced communities, and stateless people.".to_string(),
        website: "https://www.unhcr.org/".to_string(),
        contact: "contact@unhcr.org".to_string(),
        data_sources: vec![
            "unhcr-displacement".to_string(),
            "unhcr-camps".to_string(),
            "unhcr-emergencies".to_string(),
        ],
        is_verified: true,
        registered_at: Utc::now(),
        terms_url: Some("https://www.unhcr.org/terms-and-conditions.html".to_string()),
        metadata: HashMap::new(),
    }
}

/// Create a provider for ShotSpotter
pub fn create_shotspotter_provider() -> DataProvider {
    DataProvider {
        id: "shotspotter".to_string(),
        name: "ShotSpotter".to_string(),
        description: "Provider of precision policing solutions that help law enforcement officials identify, locate and deter gun violence.".to_string(),
        website: "https://www.shotspotter.com/".to_string(),
        contact: "info@shotspotter.com".to_string(),
        data_sources: vec![
            "shotspotter-incidents".to_string(),
        ],
        is_verified: true,
        registered_at: Utc::now(),
        terms_url: Some("https://www.shotspotter.com/terms-of-use/".to_string()),
        metadata: HashMap::new(),
    }
}

/// Create a provider for World Bank
pub fn create_worldbank_provider() -> DataProvider {
    DataProvider {
        id: "worldbank".to_string(),
        name: "World Bank".to_string(),
        description: "International financial institution that provides loans and grants to the governments of low- and middle-income countries.".to_string(),
        website: "https://www.worldbank.org/".to_string(),
        contact: "data@worldbank.org".to_string(),
        data_sources: vec![
            "worldbank-indicators".to_string(),
            "worldbank-climate".to_string(),
            "worldbank-poverty".to_string(),
        ],
        is_verified: true,
        registered_at: Utc::now(),
        terms_url: Some("https://www.worldbank.org/en/about/legal/terms-of-use-for-datasets".to_string()),
        metadata: HashMap::new(),
    }
}

/// Create a provider for CWOP (Citizen Weather Observer Program)
pub fn create_cwop_provider() -> DataProvider {
    DataProvider {
        id: "cwop".to_string(),
        name: "Citizen Weather Observer Program".to_string(),
        description:
            "Public-private partnership with volunteers who collect and share weather data."
                .to_string(),
        website: "https://www.weather.gov/aprfc/cwop".to_string(),
        contact: "info@wxqa.com".to_string(),
        data_sources: vec!["cwop-stations".to_string()],
        is_verified: true,
        registered_at: Utc::now(),
        terms_url: None,
        metadata: HashMap::new(),
    }
}

/// Create a NOAA Weather Service source
pub fn create_noaa_nws_source() -> DataSource {
    DataSource {
        id: "noaa-nws".to_string(),
        name: "NOAA National Weather Service".to_string(),
        description: "Official U.S. weather forecasts, warnings, meteorological products for forecasting the weather.".to_string(),
        categories: vec![
            DataCategory::Weather,
            DataCategory::Climate,
        ],
        endpoint: "https://api.weather.gov/".to_string(),
        formats: vec![
            DataFormat::JSON,
            DataFormat::XML,
        ],
        requires_auth: false,
        auth_method: None,
        added_at: Utc::now(),
        last_connected: None,
        avg_response_time_ms: Some(250),
        rate_limit: Some("5 requests per second".to_string()),
        reliability_score: 95,
        available_schemas: vec![
            "weather-observation-v1".to_string(),
            "weather-forecast-v1".to_string(),
            "weather-alert-v1".to_string(),
        ],
        metadata: HashMap::new(),
    }
}

/// Create a USGS Earthquake source
pub fn create_usgs_earthquake_source() -> DataSource {
    DataSource {
        id: "usgs-earthquake".to_string(),
        name: "USGS Earthquake Hazards Program".to_string(),
        description: "Real-time and historic earthquake data worldwide.".to_string(),
        categories: vec![DataCategory::Seismic],
        endpoint: "https://earthquake.usgs.gov/fdsnws/event/1/".to_string(),
        formats: vec![DataFormat::JSON, DataFormat::CSV, DataFormat::XML],
        requires_auth: false,
        auth_method: None,
        added_at: Utc::now(),
        last_connected: None,
        avg_response_time_ms: Some(200),
        rate_limit: Some("20000 requests per day".to_string()),
        reliability_score: 98,
        available_schemas: vec!["seismic-event-v1".to_string()],
        metadata: HashMap::new(),
    }
}

/// Create an EPA AirNow source
pub fn create_epa_airnow_source() -> DataSource {
    DataSource {
        id: "epa-airnow".to_string(),
        name: "EPA AirNow API".to_string(),
        description: "Air quality data and forecasts across the United States.".to_string(),
        categories: vec![DataCategory::AirQuality, DataCategory::Environmental],
        endpoint: "https://www.airnowapi.org/".to_string(),
        formats: vec![DataFormat::JSON, DataFormat::XML, DataFormat::CSV],
        requires_auth: true,
        auth_method: Some("API Key".to_string()),
        added_at: Utc::now(),
        last_connected: None,
        avg_response_time_ms: Some(300),
        rate_limit: Some("500 requests per hour".to_string()),
        reliability_score: 92,
        available_schemas: vec![
            "air-quality-observation-v1".to_string(),
            "air-quality-forecast-v1".to_string(),
        ],
        metadata: HashMap::new(),
    }
}

/// Create a NASA FIRMS (Fire Information for Resource Management System) source
pub fn create_nasa_firms_source() -> DataSource {
    DataSource {
        id: "nasa-firms".to_string(),
        name: "NASA FIRMS".to_string(),
        description:
            "Near real-time active fire data from MODIS and VIIRS for fire management and research."
                .to_string(),
        categories: vec![
            DataCategory::Wildfire,
            DataCategory::Satellite,
            DataCategory::Environmental,
        ],
        endpoint: "https://firms.modaps.eosdis.nasa.gov/api/".to_string(),
        formats: vec![DataFormat::JSON, DataFormat::CSV, DataFormat::GeoJSON],
        requires_auth: true,
        auth_method: Some("API Key".to_string()),
        added_at: Utc::now(),
        last_connected: None,
        avg_response_time_ms: Some(450),
        rate_limit: Some("10000 requests per day".to_string()),
        reliability_score: 90,
        available_schemas: vec!["wildfire-hotspot-v1".to_string()],
        metadata: HashMap::new(),
    }
}

/// Create a ShotSpotter Incidents source
pub fn create_shotspotter_incidents_source() -> DataSource {
    DataSource {
        id: "shotspotter-incidents".to_string(),
        name: "ShotSpotter Incidents API".to_string(),
        description: "Gunshot detection data from acoustic sensors deployed in urban areas."
            .to_string(),
        categories: vec![DataCategory::Acoustic],
        endpoint: "https://api.shotspotter.com/incidents/".to_string(),
        formats: vec![DataFormat::JSON, DataFormat::GeoJSON],
        requires_auth: true,
        auth_method: Some("OAuth 2.0".to_string()),
        added_at: Utc::now(),
        last_connected: None,
        avg_response_time_ms: Some(150),
        rate_limit: Some("100 requests per minute".to_string()),
        reliability_score: 88,
        available_schemas: vec!["gunshot-detection-v1".to_string()],
        metadata: HashMap::new(),
    }
}

/// Create an UNHCR Displacement source
pub fn create_unhcr_displacement_source() -> DataSource {
    DataSource {
        id: "unhcr-displacement".to_string(),
        name: "UNHCR Refugee Data Finder".to_string(),
        description: "Data on forcibly displaced populations, including refugees, asylum-seekers, and internally displaced people.".to_string(),
        categories: vec![
            DataCategory::Humanitarian,
        ],
        endpoint: "https://api.unhcr.org/population/".to_string(),
        formats: vec![
            DataFormat::JSON,
            DataFormat::CSV,
        ],
        requires_auth: true,
        auth_method: Some("API Key".to_string()),
        added_at: Utc::now(),
        last_connected: None,
        avg_response_time_ms: Some(350),
        rate_limit: Some("1000 requests per day".to_string()),
        reliability_score: 85,
        available_schemas: vec![
            "humanitarian-displacement-v1".to_string(),
        ],
        metadata: HashMap::new(),
    }
}

/// Create a World Bank Economic Indicators source
pub fn create_worldbank_indicators_source() -> DataSource {
    DataSource {
        id: "worldbank-indicators".to_string(),
        name: "World Bank Indicators API".to_string(),
        description: "Economic indicators and development data for countries worldwide."
            .to_string(),
        categories: vec![DataCategory::Economic],
        endpoint: "https://api.worldbank.org/v2/".to_string(),
        formats: vec![DataFormat::JSON, DataFormat::XML],
        requires_auth: false,
        auth_method: None,
        added_at: Utc::now(),
        last_connected: None,
        avg_response_time_ms: Some(400),
        rate_limit: Some("10000 requests per day".to_string()),
        reliability_score: 95,
        available_schemas: vec!["economic-indicator-v1".to_string()],
        metadata: HashMap::new(),
    }
}

/// Create a Citizen Weather Observer Program source
pub fn create_cwop_stations_source() -> DataSource {
    DataSource {
        id: "cwop-stations".to_string(),
        name: "CWOP Personal Weather Stations".to_string(),
        description: "Weather data from citizen-operated personal weather stations.".to_string(),
        categories: vec![DataCategory::Weather, DataCategory::Environmental],
        endpoint: "https://cwop.aprs.fi/".to_string(),
        formats: vec![DataFormat::CSV, DataFormat::XML],
        requires_auth: false,
        auth_method: None,
        added_at: Utc::now(),
        last_connected: None,
        avg_response_time_ms: Some(500),
        rate_limit: None,
        reliability_score: 80,
        available_schemas: vec!["weather-observation-v1".to_string()],
        metadata: HashMap::new(),
    }
}

/// Initialize all standard providers and sources
pub fn initialize_standard_providers() -> (Vec<DataProvider>, Vec<DataSource>) {
    let providers = vec![
        create_noaa_provider(),
        create_usgs_provider(),
        create_epa_provider(),
        create_nasa_provider(),
        create_esa_provider(),
        create_emsc_provider(),
        create_who_provider(),
        create_unhcr_provider(),
        create_shotspotter_provider(),
        create_worldbank_provider(),
        create_cwop_provider(),
    ];

    let sources = vec![
        create_noaa_nws_source(),
        create_usgs_earthquake_source(),
        create_epa_airnow_source(),
        create_nasa_firms_source(),
        create_shotspotter_incidents_source(),
        create_unhcr_displacement_source(),
        create_worldbank_indicators_source(),
        create_cwop_stations_source(),
    ];

    (providers, sources)
}
