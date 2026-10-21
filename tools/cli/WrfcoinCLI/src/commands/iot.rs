use anyhow::Result;
use clap::Subcommand;
use serde_json::{Value, json};
use wrfcoin_cli::{ApiClient, ApiConfig, ServiceTarget};

#[derive(Subcommand)]
pub enum IoTCommands {
    /// List available IoT sensor networks
    List,
    /// Show IoT network status
    Status {
        /// Network name
        network: Option<String>,
    },
    /// Register new sensor device
    Register {
        /// Device ID
        device_id: String,
        /// Network to register with
        network: String,
        /// Sensor type
        sensor_type: String,
    },
    /// Read sensor data
    Read {
        /// Device ID
        device_id: String,
        /// Data type (temperature, humidity, air_quality, etc.)
        data_type: String,
    },
    /// Show network coverage
    Coverage {
        /// Network name
        network: String,
        /// Location (optional)
        location: Option<String>,
    },
}

/// `iot` talks to backend's iot-sensor-networks service (port 3002), not the
/// node — see `ServiceTarget::Iot` (#3204). Only `/iot/networks` and
/// `/iot/metrics` exist there today; register/read/coverage report NOT_FOUND
/// with the exact URL instead of a silent 404 against the node.
pub async fn handle_iot_command(command: &IoTCommands) -> Result<Value> {
    let config = ApiConfig::for_service(ServiceTarget::Iot);
    let client = ApiClient::new(config.clone())?;
    let mut out = handle_iot_inner(&client, command).await?;
    ServiceTarget::Iot.attach(&config, &mut out);
    Ok(out)
}

async fn handle_iot_inner(client: &ApiClient, command: &IoTCommands) -> Result<Value> {
    match command {
        IoTCommands::List => {
            let networks = json!([
                {"name": "TTN", "description": "The Things Network", "gateways": "170K+"},
                {"name": "FOAM", "description": "Proof of Location protocol"},
                {"name": "Streamr", "description": "Real-time data streams"},
                {"name": "Sensor.Community", "sensors": "14K+"},
                {"name": "Luftdaten", "description": "Air quality monitoring"},
                {"name": "IoTeX", "description": "Blockchain-powered IoT"}
            ]);
            Ok(json!({
                "summary": "IoT network catalog (static; not a live status)",
                "catalog_only": true,
                "networks": networks
            }))
        },
        IoTCommands::Status { network: _ } => {
            // backend registers /iot/networks (+ /iot/metrics), not /iot/status.
            let result: Value = client.get("/iot/networks", None).await?;
            Ok(json!({ "summary": "IoT status retrieved", "status": result }))
        },
        IoTCommands::Register {
            device_id,
            network,
            sensor_type,
        } => {
            let result: Value = client
                .post(
                    "/iot/sensors",
                    Some(json!({
                        "deviceId": device_id,
                        "network": network,
                        "sensorType": sensor_type
                    })),
                )
                .await?;
            Ok(json!({ "summary": "Device registered", "result": result }))
        },
        IoTCommands::Read {
            device_id,
            data_type,
        } => {
            let result: Value = client
                .get(
                    &format!("/iot/readings?deviceId={}&type={}", device_id, data_type),
                    None,
                )
                .await?;
            Ok(json!({ "summary": "Sensor data retrieved", "readings": result }))
        },
        IoTCommands::Coverage { network, location } => {
            let mut path = format!("/iot/coverage?network={}", network);
            if let Some(loc) = location {
                path.push_str(&format!("&location={}", loc));
            }
            let result: Value = client.get(&path, None).await?;
            Ok(json!({ "summary": format!("Coverage for {}", network), "coverage": result }))
        },
    }
}
