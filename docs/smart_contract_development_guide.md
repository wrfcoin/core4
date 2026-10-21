# Smart Contract Development Guide

**Date Created:** May 18, 2025  
**Last Edited:** May 26, 2025

## Building Climate-Aware Contracts on Wrfcoin

## Overview

Wrfcoin supports multiple smart contract languages and execution environments, making it ideal for
building climate-aware applications. This guide covers everything from basic contracts to advanced
climate data integration, featuring 13 cross-chain bridge protocols, AI-enhanced validation systems,
and comprehensive IoT sensor network integrations.

## Supported Languages

### 1. Rust (Native)

- **Performance**: Highest performance, native blockchain integration
- **Use Case**: Core infrastructure, high-frequency trading, validators
- **Compilation**: Direct to native bytecode

### 2. WebAssembly (WASM)

- **Performance**: Near-native performance with portability
- **Use Case**: Cross-chain compatibility, third-party integrations
- **Languages**: Rust, AssemblyScript, C/C++

### 3. Solidity (EVM Compatible)

- **Performance**: Good performance, familiar to Ethereum developers
- **Use Case**: DeFi applications, NFT marketplaces, simple contracts
- **Migration**: Easy migration from Ethereum

## Development Environment Setup

### Prerequisites

```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Install Wrfcoin CLI
cargo install wrfcoin-cli

# Install development tools
cargo install cargo-generate wasm-pack
```

### Project Template

```bash
# Create new contract project
cargo generate --git https://github.com/wrfcoin/contract-template.git

# Or start from scratch
cargo new my-weather-contract --lib
cd my-weather-contract
```

### Cargo.toml Configuration

```toml
[package]
name = "weather-data-contract"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
wrfcoin-std = "0.5.2"
wrfcoin-contract = "0.5.2"
serde = { version = "1.0", features = ["derive"] }
cosmwasm-std = "1.0"

[dev-dependencies]
wrfcoin-testing = "0.5.2"
```

## Basic Contract Structure

### Simple Weather Data Contract

```rust
use wrfcoin_std::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct InstantiateMsg {
    pub owner: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    SubmitWeatherData {
        location: Location,
        temperature: f64,
        humidity: u8,
        pressure: f64,
    },
    RequestForecast {
        location: Location,
        days: u8,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    GetWeatherData { location: Location },
    GetOwner {},
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Location {
    pub latitude: f64,
    pub longitude: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct WeatherData {
    pub location: Location,
    pub temperature: f64,
    pub humidity: u8,
    pub pressure: f64,
    pub timestamp: u64,
    pub submitter: Addr,
}

// Contract state
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct State {
    pub owner: Addr,
    pub weather_data: Vec<WeatherData>,
}

// Entry points
#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    let owner = deps.api.addr_validate(&msg.owner)?;
    let state = State {
        owner,
        weather_data: vec![],
    };
    STATE.save(deps.storage, &state)?;

    Ok(Response::new()
        .add_attribute("method", "instantiate")
        .add_attribute("owner", msg.owner))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::SubmitWeatherData {
            location,
            temperature,
            humidity,
            pressure,
        } => execute_submit_weather_data(deps, env, info, location, temperature, humidity, pressure),
        ExecuteMsg::RequestForecast { location, days } => {
            execute_request_forecast(deps, env, info, location, days)
        }
    }
}

pub fn execute_submit_weather_data(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    location: Location,
    temperature: f64,
    humidity: u8,
    pressure: f64,
) -> Result<Response, ContractError> {
    let mut state = STATE.load(deps.storage)?;

    let weather_data = WeatherData {
        location,
        temperature,
        humidity,
        pressure,
        timestamp: env.block.time.seconds(),
        submitter: info.sender.clone(),
    };

    state.weather_data.push(weather_data);
    STATE.save(deps.storage, &state)?;

    Ok(Response::new()
        .add_attribute("method", "submit_weather_data")
        .add_attribute("submitter", info.sender)
        .add_attribute("temperature", temperature.to_string()))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::GetWeatherData { location } => to_binary(&query_weather_data(deps, location)?),
        QueryMsg::GetOwner {} => to_binary(&query_owner(deps)?),
    }
}

pub fn query_weather_data(deps: Deps, location: Location) -> StdResult<Vec<WeatherData>> {
    let state = STATE.load(deps.storage)?;
    let filtered_data: Vec<WeatherData> = state
        .weather_data
        .into_iter()
        .filter(|data| {
            (data.location.latitude - location.latitude).abs() < 0.01
                && (data.location.longitude - location.longitude).abs() < 0.01
        })
        .collect();

    Ok(filtered_data)
}
```

## Advanced Climate Integration

### Climate Risk Assessment Contract

```rust
use wrfcoin_std::climate::*;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ClimateRiskAssessment {
    pub location: Location,
    pub risk_factors: Vec<RiskFactor>,
    pub overall_score: u8, // 0-100
    pub valid_until: u64,
    pub assessor: Addr,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RiskFactor {
    pub factor_type: RiskType,
    pub severity: u8, // 0-10
    pub confidence: f64, // 0.0-1.0
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum RiskType {
    Flood,
    Drought,
    Hurricane,
    Wildfire,
    Earthquake,
    Tornado,
    Heatwave,
    Freeze,
}

impl ClimateRiskAssessment {
    pub fn calculate_insurance_premium(&self, base_premium: Decimal) -> Decimal {
        let risk_multiplier = match self.overall_score {
            0..=20 => Decimal::from_ratio(8u128, 10u128),   // 0.8x
            21..=40 => Decimal::one(),                       // 1.0x
            41..=60 => Decimal::from_ratio(12u128, 10u128), // 1.2x
            61..=80 => Decimal::from_ratio(15u128, 10u128), // 1.5x
            81..=100 => Decimal::from_ratio(20u128, 10u128), // 2.0x
            _ => Decimal::from_ratio(25u128, 10u128),        // 2.5x
        };

        base_premium * risk_multiplier
    }
}

// Execute climate risk assessment
pub fn execute_assess_climate_risk(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    location: Location,
) -> Result<Response, ContractError> {
    // Get historical weather data from blockchain
    let historical_data = query_historical_weather(deps.as_ref(), location.clone(), 365)?;

    // Analyze risk factors
    let mut risk_factors = Vec::new();

    // Check for flood risk
    let avg_precipitation = calculate_average_precipitation(&historical_data);
    if avg_precipitation > 150.0 {
        risk_factors.push(RiskFactor {
            factor_type: RiskType::Flood,
            severity: calculate_flood_severity(avg_precipitation),
            confidence: 0.85,
        });
    }

    // Check for drought risk
    let dry_days = count_dry_days(&historical_data);
    if dry_days > 100 {
        risk_factors.push(RiskFactor {
            factor_type: RiskType::Drought,
            severity: calculate_drought_severity(dry_days),
            confidence: 0.78,
        });
    }

    // Calculate overall risk score
    let overall_score = calculate_overall_risk_score(&risk_factors);

    let assessment = ClimateRiskAssessment {
        location,
        risk_factors,
        overall_score,
        valid_until: env.block.time.seconds() + 86400 * 30, // Valid for 30 days
        assessor: info.sender.clone(),
    };

    // Store assessment
    RISK_ASSESSMENTS.save(deps.storage, location_key(&assessment.location), &assessment)?;

    Ok(Response::new()
        .add_attribute("method", "assess_climate_risk")
        .add_attribute("location", format!("{},{}", assessment.location.latitude, assessment.location.longitude))
        .add_attribute("risk_score", overall_score.to_string()))
}
```

### Weather Derivatives Contract

```rust
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WeatherDerivative {
    pub contract_id: String,
    pub underlying: WeatherIndex,
    pub strike: f64,
    pub payout_per_unit: Decimal,
    pub expiry: u64,
    pub buyer: Addr,
    pub seller: Addr,
    pub premium: Decimal,
    pub status: ContractStatus,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum WeatherIndex {
    Temperature {
        location: Location,
        period: TimePeriod,
        measurement: TemperatureMeasurement,
    },
    Precipitation {
        location: Location,
        period: TimePeriod,
    },
    WindSpeed {
        location: Location,
        period: TimePeriod,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum TemperatureMeasurement {
    Average,
    Maximum,
    Minimum,
    HeatingDegreeDays,
    CoolingDegreeDays,
}

pub fn execute_settle_derivative(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    contract_id: String,
) -> Result<Response, ContractError> {
    let mut derivative = DERIVATIVES.load(deps.storage, &contract_id)?;

    // Check if contract has expired
    if env.block.time.seconds() < derivative.expiry {
        return Err(ContractError::ContractNotExpired {});
    }

    // Calculate actual weather index value
    let actual_value = match &derivative.underlying {
        WeatherIndex::Temperature { location, period, measurement } => {
            let weather_data = query_historical_weather(
                deps.as_ref(),
                location.clone(),
                period.days(),
            )?;
            calculate_temperature_index(&weather_data, measurement)
        }
        WeatherIndex::Precipitation { location, period } => {
            let weather_data = query_historical_weather(
                deps.as_ref(),
                location.clone(),
                period.days(),
            )?;
            calculate_precipitation_total(&weather_data)
        }
        WeatherIndex::WindSpeed { location, period } => {
            let weather_data = query_historical_weather(
                deps.as_ref(),
                location.clone(),
                period.days(),
            )?;
            calculate_average_wind_speed(&weather_data)
        }
    };

    // Calculate payout
    let payout = if actual_value > derivative.strike {
        let excess = actual_value - derivative.strike;
        derivative.payout_per_unit * Decimal::from_str(&excess.to_string())?
    } else {
        Decimal::zero()
    };

    // Update contract status
    derivative.status = ContractStatus::Settled { payout, actual_value };
    DERIVATIVES.save(deps.storage, &contract_id, &derivative)?;

    // Transfer payout if applicable
    let mut response = Response::new()
        .add_attribute("method", "settle_derivative")
        .add_attribute("contract_id", contract_id)
        .add_attribute("actual_value", actual_value.to_string())
        .add_attribute("payout", payout.to_string());

    if !payout.is_zero() {
        response = response.add_message(BankMsg::Send {
            to_address: derivative.buyer.to_string(),
            amount: coins(payout.u128(), "uwrf"),
        });
    }

    Ok(response)
}
```

## Cross-Chain Integration

### Ethereum Bridge Contract

```rust
use wrfcoin_std::bridge::*;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BridgeMessage {
    pub source_chain: String,
    pub target_chain: String,
    pub payload: Binary,
    pub nonce: u64,
    pub signatures: Vec<Signature>,
}

pub fn execute_bridge_weather_data(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    target_chain: String,
    weather_data: WeatherData,
) -> Result<Response, ContractError> {
    // Validate weather data
    validate_weather_data(&weather_data)?;

    // Create bridge message
    let payload = to_binary(&weather_data)?;
    let bridge_msg = BridgeMessage {
        source_chain: "wrfcoin".to_string(),
        target_chain: target_chain.clone(),
        payload,
        nonce: get_next_nonce(deps.storage)?,
        signatures: vec![],
    };

    // Queue for validator signing
    PENDING_BRIDGE_MESSAGES.save(deps.storage, bridge_msg.nonce, &bridge_msg)?;

    Ok(Response::new()
        .add_attribute("method", "bridge_weather_data")
        .add_attribute("target_chain", target_chain)
        .add_attribute("nonce", bridge_msg.nonce.to_string()))
}
```

## Security Best Practices

### Input Validation

```rust
pub fn validate_location(location: &Location) -> Result<(), ContractError> {
    if location.latitude < -90.0 || location.latitude > 90.0 {
        return Err(ContractError::InvalidLatitude {});
    }
    if location.longitude < -180.0 || location.longitude > 180.0 {
        return Err(ContractError::InvalidLongitude {});
    }
    Ok(())
}

pub fn validate_weather_data(data: &WeatherData) -> Result<(), ContractError> {
    validate_location(&data.location)?;

    if data.temperature < -100.0 || data.temperature > 100.0 {
        return Err(ContractError::InvalidTemperature {});
    }

    if data.humidity > 100 {
        return Err(ContractError::InvalidHumidity {});
    }

    if data.pressure < 800.0 || data.pressure > 1200.0 {
        return Err(ContractError::InvalidPressure {});
    }

    Ok(())
}
```

### Access Control

```rust
pub fn only_owner(deps: Deps, info: &MessageInfo) -> Result<(), ContractError> {
    let state = STATE.load(deps.storage)?;
    if info.sender != state.owner {
        return Err(ContractError::Unauthorized {});
    }
    Ok(())
}

pub fn only_validator(deps: Deps, info: &MessageInfo) -> Result<(), ContractError> {
    let validators = VALIDATORS.load(deps.storage)?;
    if !validators.contains(&info.sender) {
        return Err(ContractError::NotValidator {});
    }
    Ok(())
}
```

### Reentrancy Protection

```rust
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ReentrancyGuard {
    pub locked: bool,
}

pub fn with_reentrancy_guard<F, T>(
    deps: DepsMut,
    f: F,
) -> Result<T, ContractError>
where
    F: FnOnce(DepsMut) -> Result<T, ContractError>,
{
    let mut guard = REENTRANCY_GUARD.may_load(deps.storage)?.unwrap_or(ReentrancyGuard { locked: false });

    if guard.locked {
        return Err(ContractError::ReentrancyGuard {});
    }

    guard.locked = true;
    REENTRANCY_GUARD.save(deps.storage, &guard)?;

    let result = f(deps);

    let mut guard = REENTRANCY_GUARD.load(deps.storage)?;
    guard.locked = false;
    REENTRANCY_GUARD.save(deps.storage, &guard)?;

    result
}
```

## Testing Framework

### Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info};
    use cosmwasm_std::{coins, from_binary};

    #[test]
    fn test_instantiate() {
        let mut deps = mock_dependencies();
        let msg = InstantiateMsg {
            owner: "owner".to_string(),
        };
        let info = mock_info("creator", &coins(1000, "earth"));
        let res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();
        assert_eq!(0, res.messages.len());
    }

    #[test]
    fn test_submit_weather_data() {
        let mut deps = mock_dependencies();
        let msg = InstantiateMsg {
            owner: "owner".to_string(),
        };
        let info = mock_info("creator", &coins(2, "token"));
        let _res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();

        let location = Location {
            latitude: 40.7128,
            longitude: -74.0060,
        };

        let msg = ExecuteMsg::SubmitWeatherData {
            location: location.clone(),
            temperature: 22.5,
            humidity: 65,
            pressure: 1013.25,
        };
        let info = mock_info("submitter", &coins(0, "token"));
        let res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();

        assert_eq!(res.attributes[0].value, "submit_weather_data");

        // Query the data back
        let query_msg = QueryMsg::GetWeatherData { location };
        let res = query(deps.as_ref(), mock_env(), query_msg).unwrap();
        let weather_data: Vec<WeatherData> = from_binary(&res).unwrap();
        assert_eq!(weather_data.len(), 1);
        assert_eq!(weather_data[0].temperature, 22.5);
    }
}
```

### Integration Tests

```rust
#[cfg(test)]
mod integration_tests {
    use super::*;
    use wrfcoin_testing::*;

    #[test]
    fn test_climate_risk_assessment_integration() {
        let mut app = WrfcoinApp::new();

        // Deploy contract
        let contract_addr = app.deploy_contract("weather-contract.wasm").unwrap();

        // Submit historical weather data
        for i in 0..365 {
            app.execute_contract(
                contract_addr.clone(),
                ExecuteMsg::SubmitWeatherData {
                    location: Location { latitude: 40.7128, longitude: -74.0060 },
                    temperature: 20.0 + (i as f64 * 0.1),
                    humidity: 60,
                    pressure: 1013.25,
                },
            ).unwrap();
        }

        // Request climate risk assessment
        let result = app.execute_contract(
            contract_addr.clone(),
            ExecuteMsg::AssessClimateRisk {
                location: Location { latitude: 40.7128, longitude: -74.0060 },
            },
        ).unwrap();

        assert!(result.events.iter().any(|e| e.ty == "climate_risk_assessed"));
    }
}
```

## Deployment

### Compile Contract

```bash
# Optimize for production
cargo wasm
wasm-opt -Oz target/wasm32-unknown-unknown/release/weather_contract.wasm -o weather_contract_optimized.wasm

# Verify contract
wrfcoin-cli contract verify weather_contract_optimized.wasm
```

### Deploy to Testnet

```bash
# Store contract code
wrfcoin-cli tx wasm store weather_contract_optimized.wasm \
  --from deployer \
  --chain-id wrfcoin-testnet-1 \
  --gas auto \
  --gas-adjustment 1.3

# Instantiate contract
wrfcoin-cli tx wasm instantiate 1 '{"owner":"wrf1..."}' \
  --from deployer \
  --label "Weather Data Contract" \
  --chain-id wrfcoin-testnet-1 \
  --gas auto
```

### Deploy to Mainnet

```bash
# Multi-sig deployment for security
wrfcoin-cli tx wasm store weather_contract_optimized.wasm \
  --from multisig \
  --chain-id wrfcoin-mainnet-1 \
  --gas 2000000 \
  --generate-only > deploy_tx.json

# Sign by multiple parties
wrfcoin-cli tx sign deploy_tx.json --from signer1 --chain-id wrfcoin-mainnet-1
wrfcoin-cli tx sign deploy_tx.json --from signer2 --chain-id wrfcoin-mainnet-1

# Broadcast signed transaction
wrfcoin-cli tx broadcast deploy_tx_signed.json
```

This comprehensive guide covers smart contract development from basic concepts to advanced climate
integration patterns, providing everything needed to build production-ready climate-aware
applications on Wrfcoin.
