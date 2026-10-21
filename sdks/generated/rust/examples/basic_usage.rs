use wrfcoin_sdk::apis::configuration::Configuration;
use wrfcoin_sdk::apis::{blockchain_api, wallet_api, weather_data_api};
use wrfcoin_sdk::models::{WeatherDataSubmission, WeatherDataSubmissionSensorDataInner};

/**
 * WRFCoin Rust SDK - Basic Usage Example
 *
 * This example demonstrates how to use the WRFCoin Rust SDK
 * to interact with the WRFCoin blockchain platform.
 */

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🌍 WRFCoin SDK Example - Rust");
    println!("===============================\n");

    // Configure the API client
    let mut config = Configuration::new();
    config.base_path = "http://localhost:3001".to_string(); // Development server
    // For production: https://api.wrfcoin.com/v1
    // config.api_key = Some("your-api-key-here".to_string());

    // Run all examples
    if let Err(e) = run_examples(&config).await {
        eprintln!("❌ Error occurred: {}", e);
        return Err(e);
    }

    println!("✅ All examples completed successfully!");
    Ok(())
}

async fn run_examples(config: &Configuration) -> Result<(), Box<dyn std::error::Error>> {
    // 1. Get blockchain status
    get_blockchain_status(config).await?;

    // 2. Get recent blocks
    get_recent_blocks(config).await?;

    // 3. Generate wallet mnemonic
    generate_wallet_mnemonic(config).await?;

    // 4. Submit weather data
    submit_weather_data(config).await?;

    Ok(())
}

async fn get_blockchain_status(config: &Configuration) -> Result<(), Box<dyn std::error::Error>> {
    println!("1. Fetching blockchain status...");

    match blockchain_api::api_blockchain_status_get(config).await {
        Ok(response) => {
            println!("   Blockchain Status: {:?}", response);
            // Access specific fields if available
            println!("   Status retrieved successfully");
        },
        Err(e) => {
            println!("   Error getting blockchain status: {}", e);
        },
    }
    println!();
    Ok(())
}

async fn get_recent_blocks(config: &Configuration) -> Result<(), Box<dyn std::error::Error>> {
    println!("2. Fetching recent blocks...");

    match blockchain_api::api_blockchain_blocks_get(config, Some(5), None, None).await {
        Ok(response) => {
            println!("   Blocks response: {:?}", response);
            println!("   Retrieved recent blocks successfully");
        },
        Err(e) => {
            println!("   Error getting blocks: {}", e);
        },
    }
    println!();
    Ok(())
}

async fn generate_wallet_mnemonic(
    config: &Configuration,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("3. Generating new wallet mnemonic...");

    match wallet_api::api_wallet_generate_mnemonic_post(config).await {
        Ok(response) => {
            println!("   Mnemonic generated successfully");
            // Note: In production, handle mnemonic securely and never log it
            println!("   Response: {:?}", response);
        },
        Err(e) => {
            println!("   Error generating mnemonic: {}", e);
        },
    }
    println!();
    Ok(())
}

async fn submit_weather_data(config: &Configuration) -> Result<(), Box<dyn std::error::Error>> {
    println!("4. Submitting weather data...");

    // Create weather data submission
    let weather_data = create_sample_weather_data();

    match weather_data_api::api_weather_submit_post(config, weather_data).await {
        Ok(response) => {
            println!("   Weather data submitted successfully");
            println!("   Response: {:?}", response);
        },
        Err(e) => {
            println!("   Error submitting weather data: {}", e);
        },
    }
    println!();
    Ok(())
}

fn create_sample_weather_data() -> WeatherDataSubmission {
    WeatherDataSubmission {
        station_id: "example-station-001".to_string(),
        timestamp: Some(chrono::Utc::now().timestamp() as i32),
        location: None,
        measurements: None,
        signature: None,
        sensor_data: create_sensor_data(),
        quality: None,
    }
}

fn create_sensor_data() -> Vec<WeatherDataSubmissionSensorDataInner> {
    vec![WeatherDataSubmissionSensorDataInner::default()]
}

/// Demonstrate error handling patterns
pub async fn demonstrate_error_handling(config: &Configuration) {
    println!("6. Demonstrating error handling...");

    // Intentionally make a bad request to demonstrate error handling
    match blockchain_api::api_blockchain_blocks_get(config, Some(-1), None, None).await {
        Ok(_) => {
            println!("   Unexpected success with invalid parameter");
        },
        Err(e) => {
            println!("   Caught expected error: {}", e);

            // Handle different types of errors
            match e {
                wrfcoin_sdk::apis::Error::Reqwest(reqwest_error) => {
                    println!("   Network error: {}", reqwest_error);
                },
                wrfcoin_sdk::apis::Error::Serde(serde_error) => {
                    println!("   Serialization error: {}", serde_error);
                },
                wrfcoin_sdk::apis::Error::Io(io_error) => {
                    println!("   IO error: {}", io_error);
                },
                _ => {
                    println!("   Other error type");
                },
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_configuration_creation() {
        let config = Configuration::new();
        assert!(!config.base_path.is_empty());
    }

    #[test]
    fn test_weather_data_creation() {
        let weather_data = create_sample_weather_data();
        assert_eq!(weather_data.station_id, "example-station-001");
        assert!(!weather_data.sensor_data.is_empty());
    }
}
