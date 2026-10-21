use serde::{Deserialize, Serialize};

use crate::blockchain::state::ComputationalNFT as StateComputationalNft;
use crate::common::types::{Address, Balance, NFTId, NFTMetadata, Timestamp};
use crate::weather::validation::WeatherObservation;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NftExecutionState {
    Pending,
    Active,
    Processing,
    Completed,
    Failed,
    Canceled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NftExecutionRecord {
    pub nft_id: NFTId,
    pub state: NftExecutionState,
    pub updated_at_unix: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_hash: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum NftExecutionError {
    #[error("invalid state transition from {from:?} to {to:?}")]
    InvalidStateTransition {
        from: NftExecutionState,
        to: NftExecutionState,
    },
    #[error("invalid nft id")]
    InvalidNftId,
    #[error("station_id is required")]
    InvalidStationId,
    #[error("model_name is required")]
    InvalidModelName,
}

pub fn can_transition_state(from: NftExecutionState, to: NftExecutionState) -> bool {
    matches!(
        (from, to),
        (NftExecutionState::Pending, NftExecutionState::Active)
            | (NftExecutionState::Pending, NftExecutionState::Canceled)
            | (NftExecutionState::Active, NftExecutionState::Processing)
            | (NftExecutionState::Active, NftExecutionState::Canceled)
            | (NftExecutionState::Processing, NftExecutionState::Completed)
            | (NftExecutionState::Processing, NftExecutionState::Failed)
            | (NftExecutionState::Failed, NftExecutionState::Processing)
    )
}

pub fn transition_execution_state(
    record: &mut NftExecutionRecord,
    to: NftExecutionState,
    result_hash: Option<String>,
    updated_at_unix: u64,
) -> Result<(), NftExecutionError> {
    if !can_transition_state(record.state, to) {
        return Err(NftExecutionError::InvalidStateTransition {
            from: record.state,
            to,
        });
    }
    record.state = to;
    record.updated_at_unix = updated_at_unix;
    record.result_hash = result_hash;
    Ok(())
}

pub struct WeatherModelMintInput<'a> {
    pub nft_id: &'a str,
    pub owner: Address,
    pub creator: Address,
    pub station_id: &'a str,
    pub model_name: &'a str,
    pub observed_at_unix: u64,
    pub observation: &'a WeatherObservation,
    pub price: Option<Balance>,
    /// Deterministic, block-derived creation timestamp (unix seconds). Stored in
    /// both `created_at` and `updated_at`, which are hashed into the state_root,
    /// so it must NOT be wall-clock — see #1968.
    pub created_at_unix: u64,
}

pub fn weather_model_output_metadata(
    creator: Address,
    station_id: &str,
    model_name: &str,
    observation: &WeatherObservation,
    observed_at_unix: u64,
) -> NFTMetadata {
    NFTMetadata {
        name: format!("Weather Model Output ({model_name})"),
        description: Some(format!("Station {station_id} weather model output")),
        data_type: "weather_model_output".to_string(),
        region: None,
        accuracy: None,
        creation_time: Timestamp::new(observed_at_unix),
        creator,
        metadata_uri: None,
        properties: serde_json::json!({
            "station_id": station_id,
            "model_name": model_name,
            "observed_at_unix": observed_at_unix,
            "observation": observation,
        }),
    }
}

pub fn build_weather_model_output_nft(
    input: WeatherModelMintInput<'_>,
) -> Result<StateComputationalNft, NftExecutionError> {
    if input.nft_id.trim().is_empty() {
        return Err(NftExecutionError::InvalidNftId);
    }
    if input.station_id.trim().is_empty() {
        return Err(NftExecutionError::InvalidStationId);
    }
    if input.model_name.trim().is_empty() {
        return Err(NftExecutionError::InvalidModelName);
    }

    // created_at records when this NFT record was created, not the observation time.
    // The observation anchor is stored in metadata.creation_time and
    // metadata.properties["observed_at_unix"]. Sourced deterministically from the
    // block timestamp (never wall-clock) because it is hashed into the state_root
    // (#1968).
    let created_at = input.created_at_unix;

    Ok(StateComputationalNft {
        id: NFTId::new(input.nft_id.to_string()),
        owner: input.owner,
        creator: input.creator,
        metadata: weather_model_output_metadata(
            input.creator,
            input.station_id,
            input.model_name,
            input.observation,
            input.observed_at_unix,
        ),
        price: input.price,
        verified: false,
        verification_count: 0,
        avg_confidence: None,
        created_at,
        updated_at: created_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn execution_state_transitions_follow_expected_graph() {
        let mut record = NftExecutionRecord {
            nft_id: NFTId::new("nft-1".to_string()),
            state: NftExecutionState::Pending,
            updated_at_unix: 100,
            result_hash: None,
        };
        transition_execution_state(&mut record, NftExecutionState::Active, None, 110).unwrap();
        transition_execution_state(&mut record, NftExecutionState::Processing, None, 120).unwrap();
        transition_execution_state(
            &mut record,
            NftExecutionState::Completed,
            Some("hash-1".to_string()),
            130,
        )
        .unwrap();
        assert_eq!(record.state, NftExecutionState::Completed);
        assert_eq!(record.result_hash.as_deref(), Some("hash-1"));
    }

    #[test]
    fn invalid_state_transition_is_rejected() {
        let mut record = NftExecutionRecord {
            nft_id: NFTId::new("nft-2".to_string()),
            state: NftExecutionState::Pending,
            updated_at_unix: 100,
            result_hash: None,
        };
        let err = transition_execution_state(&mut record, NftExecutionState::Completed, None, 110)
            .expect_err("pending -> completed must fail");
        assert!(matches!(
            err,
            NftExecutionError::InvalidStateTransition { .. }
        ));
    }

    #[test]
    fn build_weather_model_output_nft_populates_metadata() {
        let owner = Address::new([1; 20]);
        let creator = Address::new([2; 20]);
        let observation = WeatherObservation {
            temperature_c: Some(21.5),
            humidity_percent: Some(40.0),
            pressure_hpa: Some(1012.4),
            wind_speed_ms: Some(3.2),
            precipitation_mm: Some(0.0),
            dewpoint_c: Some(7.0),
            ..Default::default()
        };

        let nft = build_weather_model_output_nft(WeatherModelMintInput {
            nft_id: "nft-weather-1",
            owner,
            creator,
            station_id: "station-001",
            model_name: "wrf-v1",
            observed_at_unix: 1_700_000_000,
            observation: &observation,
            price: Some(Balance::new(42)),
            created_at_unix: 1_700_000_500,
        })
        .unwrap();

        assert_eq!(nft.id.as_str(), "nft-weather-1");
        assert_eq!(nft.owner, owner);
        assert_eq!(nft.creator, creator);
        assert_eq!(nft.price, Some(Balance::new(42)));
        assert_eq!(nft.metadata.data_type, "weather_model_output");
        assert_eq!(
            nft.metadata.properties["station_id"].as_str(),
            Some("station-001")
        );
        assert_eq!(
            nft.metadata.properties["model_name"].as_str(),
            Some("wrf-v1")
        );
        // The creation timestamp must be the deterministic, block-derived value
        // passed in — never a wall-clock reading (#1968).
        assert_eq!(nft.created_at, 1_700_000_500);
        assert_eq!(nft.updated_at, 1_700_000_500);
    }
}
