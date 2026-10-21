use crate::weather::validation::{
    WeatherFieldKeyMode, WeatherObservation, weather_observation_from_json,
    weather_observation_from_json_with_key_mode,
};
pub use crate::weather::validation::{
    ParsedWeatherPayload, WEATHER_SCHEMA_V1, WeatherPayloadClassification,
    WeatherPayloadParseOptions, WeatherPayloadSchemaVersion,
};

pub fn parse_weather_payload(
    data: &serde_json::Value,
) -> Result<Option<ParsedWeatherPayload>, String> {
    parse_weather_payload_with_options(data, WeatherPayloadParseOptions::default())
}

pub fn parse_weather_payload_with_options(
    data: &serde_json::Value,
    options: WeatherPayloadParseOptions,
) -> Result<Option<ParsedWeatherPayload>, String> {
    let object = match data.as_object() {
        Some(object) => object,
        None => return Ok(None),
    };

    let schema_version = extract_schema_version(object)?;

    match schema_version {
        Some(WEATHER_SCHEMA_V1) => parse_v1_payload(object, options).map(Some),
        Some(other) => Err(format!(
            "unsupported weather payload schema version: {other} (supported: {WEATHER_SCHEMA_V1})"
        )),
        None => parse_legacy_payload(data, object),
    }
}

pub fn classify_weather_payload(
    data: &serde_json::Value,
    options: WeatherPayloadParseOptions,
) -> WeatherPayloadClassification {
    match parse_weather_payload_with_options(data, options) {
        Ok(Some(parsed)) => match parsed.version {
            WeatherPayloadSchemaVersion::V1 => WeatherPayloadClassification::CanonicalV1,
            WeatherPayloadSchemaVersion::LegacyUnversioned => {
                WeatherPayloadClassification::LegacyUnversioned
            },
        },
        Ok(None) => WeatherPayloadClassification::NonWeather,
        Err(error) if error.contains("unsupported weather payload schema version") => {
            WeatherPayloadClassification::UnsupportedSchemaVersion
        },
        Err(error)
            if error.contains("schema version marker") && error.contains("must be a string") =>
        {
            WeatherPayloadClassification::InvalidSchemaVersionMarkerType
        },
        Err(_) => WeatherPayloadClassification::InvalidCanonicalPayload,
    }
}

pub fn canonicalize_weather_payload(
    data: &serde_json::Value,
    options: WeatherPayloadParseOptions,
) -> Result<Option<serde_json::Value>, String> {
    let Some(parsed) = parse_weather_payload_with_options(data, options)? else {
        return Ok(None);
    };

    let mut output = serde_json::Map::new();
    output.insert(
        "schema_version".to_string(),
        serde_json::Value::String(WEATHER_SCHEMA_V1.to_string()),
    );

    if let Some(object) = data.as_object() {
        if let Some(station_id) = object
            .get("station_id")
            .or_else(|| object.get("stationId"))
            .and_then(serde_json::Value::as_str)
        {
            output.insert(
                "station_id".to_string(),
                serde_json::Value::String(station_id.to_string()),
            );
        }
        if let Some(observed_at_unix) = object
            .get("observed_at_unix")
            .or_else(|| object.get("observedAtUnix"))
            .and_then(as_i64_value)
        {
            output.insert(
                "observed_at_unix".to_string(),
                serde_json::Value::Number(observed_at_unix.into()),
            );
        }
    }

    output.insert(
        "observation".to_string(),
        canonical_observation_json(&parsed.observation),
    );

    Ok(Some(serde_json::Value::Object(output)))
}

fn parse_v1_payload(
    object: &serde_json::Map<String, serde_json::Value>,
    options: WeatherPayloadParseOptions,
) -> Result<ParsedWeatherPayload, String> {
    let observation_value = object
        .get("observation")
        .ok_or_else(|| "weather schema v1 requires an 'observation' object".to_string())?;
    let key_mode = if options.strict_v1_observation_keys {
        WeatherFieldKeyMode::CanonicalV1Only
    } else {
        WeatherFieldKeyMode::LegacyCompatible
    };
    let observation = weather_observation_from_json_with_key_mode(observation_value, key_mode)
        .ok_or_else(|| {
            if options.strict_v1_observation_keys {
                "weather schema v1 observation must include at least one recognized canonical weather field"
                    .to_string()
            } else {
                "weather schema v1 observation must include at least one recognized weather field".to_string()
            }
        })?;

    Ok(ParsedWeatherPayload {
        version: WeatherPayloadSchemaVersion::V1,
        observation,
    })
}

fn parse_legacy_payload(
    data: &serde_json::Value,
    object: &serde_json::Map<String, serde_json::Value>,
) -> Result<Option<ParsedWeatherPayload>, String> {
    if let Some(observation) = weather_observation_from_json(data) {
        return Ok(Some(ParsedWeatherPayload {
            version: WeatherPayloadSchemaVersion::LegacyUnversioned,
            observation,
        }));
    }

    if let Some(observation_value) = object.get("observation") {
        if let Some(observation) = weather_observation_from_json(observation_value) {
            return Ok(Some(ParsedWeatherPayload {
                version: WeatherPayloadSchemaVersion::LegacyUnversioned,
                observation,
            }));
        }
    }

    Ok(None)
}

fn extract_schema_version(
    object: &serde_json::Map<String, serde_json::Value>,
) -> Result<Option<&str>, String> {
    for key in ["schema_version", "schema", "weather_schema_version"] {
        if let Some(value) = object.get(key) {
            let Some(schema_version) = value.as_str() else {
                return Err(format!(
                    "weather payload schema version marker '{key}' must be a string"
                ));
            };
            return Ok(Some(schema_version));
        }
    }

    Ok(None)
}

fn as_i64_value(value: &serde_json::Value) -> Option<i64> {
    match value {
        serde_json::Value::Number(number) => number.as_i64(),
        serde_json::Value::String(raw) => raw.parse::<i64>().ok(),
        _ => None,
    }
}

fn canonical_observation_json(observation: &WeatherObservation) -> serde_json::Value {
    let mut map = serde_json::Map::new();

    if let Some(value) = observation.temperature_c {
        map.insert("temperature_c".to_string(), serde_json::json!(value));
    }
    if let Some(value) = observation.humidity_percent {
        map.insert("humidity".to_string(), serde_json::json!(value));
    }
    if let Some(value) = observation.pressure_hpa {
        map.insert("pressure_hpa".to_string(), serde_json::json!(value));
    }
    if let Some(value) = observation.wind_speed_ms {
        map.insert("wind_speed_ms".to_string(), serde_json::json!(value));
    }
    if let Some(value) = observation.precipitation_mm {
        map.insert("precipitation".to_string(), serde_json::json!(value));
    }
    if let Some(value) = observation.dewpoint_c {
        map.insert("dewpoint_c".to_string(), serde_json::json!(value));
    }

    serde_json::Value::Object(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_accepts_legacy_unversioned_top_level_fields() {
        let payload = serde_json::json!({
            "temperature": 21.2,
            "humidity": 56.0
        });

        let parsed = parse_weather_payload(&payload)
            .expect("legacy payload should parse")
            .expect("weather payload should be detected");

        assert_eq!(
            parsed.version,
            WeatherPayloadSchemaVersion::LegacyUnversioned
        );
        assert_eq!(parsed.observation.temperature_c, Some(21.2));
    }

    #[test]
    fn parse_accepts_v1_payload_with_observation_object() {
        let payload = serde_json::json!({
            "schema_version": WEATHER_SCHEMA_V1,
            "station_id": "station-9",
            "observed_at_unix": 1_700_200_000i64,
            "observation": {
                "temperature_c": 24.1,
                "humidity": 63.0,
                "pressure_hpa": 1007.3
            }
        });

        let parsed = parse_weather_payload(&payload)
            .expect("v1 payload should parse")
            .expect("weather payload should be detected");

        assert_eq!(parsed.version, WeatherPayloadSchemaVersion::V1);
        assert_eq!(parsed.observation.temperature_c, Some(24.1));
        assert_eq!(parsed.observation.humidity_percent, Some(63.0));
    }

    #[test]
    fn parse_rejects_non_string_schema_version_marker() {
        let payload = serde_json::json!({
            "schema_version": 1,
            "observation": {
                "temperature_c": 20.0
            }
        });

        let err =
            parse_weather_payload(&payload).expect_err("non-string version marker should fail");
        assert!(err.contains("schema version marker"));
        assert!(err.contains("must be a string"));
    }

    #[test]
    fn parse_accepts_v1_alias_observation_keys_in_default_mode() {
        let payload = serde_json::json!({
            "schema_version": WEATHER_SCHEMA_V1,
            "observation": {
                "temp_c": 24.1,
                "relative_humidity": 63.0,
                "pressure": 1007.3
            }
        });

        let parsed =
            parse_weather_payload_with_options(&payload, WeatherPayloadParseOptions::default())
                .expect("default parse mode should allow migration aliases")
                .expect("weather payload should be detected");

        assert_eq!(parsed.version, WeatherPayloadSchemaVersion::V1);
        assert_eq!(parsed.observation.temperature_c, Some(24.1));
        assert_eq!(parsed.observation.humidity_percent, Some(63.0));
    }

    #[test]
    fn parse_rejects_v1_alias_observation_keys_in_strict_mode() {
        let payload = serde_json::json!({
            "schema_version": WEATHER_SCHEMA_V1,
            "observation": {
                "temp_c": 24.1,
                "relative_humidity": 63.0
            }
        });

        let err = parse_weather_payload_with_options(
            &payload,
            WeatherPayloadParseOptions::strict_v1_keys(),
        )
        .expect_err("strict v1 mode should reject alias-only observation payload");
        assert!(err.contains("recognized canonical weather field"));
    }

    #[test]
    fn parse_strict_mode_still_accepts_legacy_unversioned_payloads() {
        let payload = serde_json::json!({
            "temperature": 21.2,
            "humidity": 56.0
        });

        let parsed = parse_weather_payload_with_options(
            &payload,
            WeatherPayloadParseOptions::strict_v1_keys(),
        )
        .expect("legacy unversioned payload should parse")
        .expect("weather payload should be detected");

        assert_eq!(
            parsed.version,
            WeatherPayloadSchemaVersion::LegacyUnversioned
        );
        assert_eq!(parsed.observation.temperature_c, Some(21.2));
    }

    #[test]
    fn parse_rejects_unknown_schema_version() {
        let payload = serde_json::json!({
            "schema_version": "wrf.weather.observation.v2",
            "observation": {
                "temperature_c": 20.0
            }
        });

        let err = parse_weather_payload(&payload).expect_err("unknown schema version should fail");
        assert!(err.contains("unsupported weather payload schema version"));
    }

    #[test]
    fn parse_rejects_v1_payload_missing_observation() {
        let payload = serde_json::json!({
            "schema_version": WEATHER_SCHEMA_V1,
            "temperature_c": 20.0
        });

        let err = parse_weather_payload(&payload).expect_err("missing observation should fail");
        assert!(err.contains("requires an 'observation' object"));
    }

    #[test]
    fn classify_payload_detects_known_categories() {
        let canonical = serde_json::json!({
            "schema_version": WEATHER_SCHEMA_V1,
            "observation": {"temperature_c": 20.0}
        });
        let legacy = serde_json::json!({"temperature": 20.0});
        let unknown = serde_json::json!({
            "schema_version": "wrf.weather.observation.v2",
            "observation": {"temperature_c": 20.0}
        });

        assert_eq!(
            classify_weather_payload(&canonical, WeatherPayloadParseOptions::default()),
            WeatherPayloadClassification::CanonicalV1
        );
        assert_eq!(
            classify_weather_payload(&legacy, WeatherPayloadParseOptions::default()),
            WeatherPayloadClassification::LegacyUnversioned
        );
        assert_eq!(
            classify_weather_payload(&unknown, WeatherPayloadParseOptions::default()),
            WeatherPayloadClassification::UnsupportedSchemaVersion
        );
    }

    #[test]
    fn canonicalize_legacy_payload_to_v1_shape() {
        let legacy = serde_json::json!({
            "station_id": "station-9",
            "observed_at_unix": 1_700_200_000i64,
            "temperature": 20.0,
            "humidity": 55.0,
            "windSpeed": 3.1
        });

        let canonical =
            canonicalize_weather_payload(&legacy, WeatherPayloadParseOptions::default())
                .expect("canonicalization should not fail")
                .expect("legacy payload should produce canonical output");

        assert_eq!(canonical["schema_version"], WEATHER_SCHEMA_V1);
        assert_eq!(canonical["station_id"], "station-9");
        assert_eq!(canonical["observation"]["temperature_c"], 20.0);
        assert_eq!(canonical["observation"]["humidity"], 55.0);
        assert_eq!(canonical["observation"]["wind_speed_ms"], 3.1);
    }

    #[test]
    fn canonicalize_maps_metadata_aliases_and_nested_observation_aliases() {
        let legacy = serde_json::json!({
            "stationId": "station-legacy",
            "observedAtUnix": "1700209999",
            "observation": {
                "temp_c": 18.25,
                "dew_point": 11.5
            }
        });

        let canonical =
            canonicalize_weather_payload(&legacy, WeatherPayloadParseOptions::default())
                .expect("canonicalization should not fail")
                .expect("legacy payload should produce canonical output");

        assert_eq!(
            canonical,
            serde_json::json!({
                "schema_version": WEATHER_SCHEMA_V1,
                "station_id": "station-legacy",
                "observed_at_unix": 1700209999i64,
                "observation": {
                    "temperature_c": 18.25,
                    "dewpoint_c": 11.5
                }
            })
        );
    }
}
