#![no_main]

//! Fuzz target for the JSON parse + `validate_weather_observation()` pipeline.
//!
//! Feeds arbitrary bytes as a JSON value through `weather_observation_from_json()`
//! then immediately validates the parsed observation.  This exercises:
//! - All JSON key alias mappings (legacy + canonical V1)
//! - NaN / Inf / subnormal f64 values that JSON parses accept as numbers
//! - Cross-parameter consistency checks (dew-point vs. temperature)
//! - Range scoring arithmetic on pathological inputs
//!
//! The target asserts only that neither function panics on any JSON input.

use libfuzzer_sys::fuzz_target;
use wrfcoin_shared_protocol::weather::validation::{
    validate_weather_observation, weather_observation_from_json,
};

fuzz_target!(|data: &[u8]| {
    if let Ok(val) = serde_json::from_slice::<serde_json::Value>(data) {
        if let Some(obs) = weather_observation_from_json(&val) {
            let _ = validate_weather_observation(&obs);
        }
    }
});
