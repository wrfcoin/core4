use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::process;
use wrfcoin_shared_protocol::weather::schema::{
    WeatherPayloadClassification, WeatherPayloadParseOptions, canonicalize_weather_payload,
    classify_weather_payload,
};

#[derive(Debug)]
struct InputRecord {
    payload: Value,
    station_id: Option<String>,
    client_id: Option<String>,
}

#[derive(Debug, Default, Clone, Serialize, PartialEq)]
struct ClassificationCounts {
    canonical_v1: u64,
    legacy_unversioned: u64,
    unsupported_schema_version: u64,
    invalid_schema_version_marker_type: u64,
    invalid_canonical_payload: u64,
    non_weather: u64,
}

impl ClassificationCounts {
    fn increment(&mut self, classification: WeatherPayloadClassification) {
        match classification {
            WeatherPayloadClassification::CanonicalV1 => self.canonical_v1 += 1,
            WeatherPayloadClassification::LegacyUnversioned => self.legacy_unversioned += 1,
            WeatherPayloadClassification::UnsupportedSchemaVersion => {
                self.unsupported_schema_version += 1;
            },
            WeatherPayloadClassification::InvalidSchemaVersionMarkerType => {
                self.invalid_schema_version_marker_type += 1;
            },
            WeatherPayloadClassification::InvalidCanonicalPayload => {
                self.invalid_canonical_payload += 1;
            },
            WeatherPayloadClassification::NonWeather => self.non_weather += 1,
        }
    }

    fn weather_total(&self) -> u64 {
        self.canonical_v1
            + self.legacy_unversioned
            + self.unsupported_schema_version
            + self.invalid_schema_version_marker_type
            + self.invalid_canonical_payload
    }
}

#[derive(Debug, Serialize, PartialEq)]
struct AdoptionPercentages {
    canonical_v1_of_weather: f64,
    legacy_unversioned_of_weather: f64,
    unsupported_or_invalid_of_weather: f64,
}

#[derive(Debug, Serialize, PartialEq)]
struct MigrationReport {
    input_file: String,
    total_records: u64,
    classification: ClassificationCounts,
    adoption_percentages: AdoptionPercentages,
    by_station: BTreeMap<String, ClassificationCounts>,
    by_client: BTreeMap<String, ClassificationCounts>,
}

fn main() {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if args.is_empty() {
        print_usage_and_exit();
    }

    let command = args[0].as_str();
    match command {
        "report" => {
            if args.len() < 2 {
                eprintln!("missing input path for report command");
                print_usage_and_exit();
            }
            let input_path = &args[1];
            let output_path = args.get(2).cloned();
            let records = read_input_records(input_path).unwrap_or_else(|error| {
                eprintln!("failed to read input records: {error}");
                process::exit(1);
            });
            let report = build_report(input_path, &records);
            write_json_output(&report, output_path.as_deref()).unwrap_or_else(|error| {
                eprintln!("failed to write report output: {error}");
                process::exit(1);
            });
        },
        "canonicalize" => {
            if args.len() < 2 {
                eprintln!("missing input path for canonicalize command");
                print_usage_and_exit();
            }
            let input_path = &args[1];
            let output_path = args.get(2).cloned();
            let records = read_input_records(input_path).unwrap_or_else(|error| {
                eprintln!("failed to read input records: {error}");
                process::exit(1);
            });
            let canonicalized =
                canonicalize_records(&records, WeatherPayloadParseOptions::default());
            write_json_output(&canonicalized, output_path.as_deref()).unwrap_or_else(|error| {
                eprintln!("failed to write canonicalization output: {error}");
                process::exit(1);
            });
        },
        _ => {
            eprintln!("unknown command: {command}");
            print_usage_and_exit();
        },
    }
}

fn print_usage_and_exit() -> ! {
    eprintln!(
        "Usage:
  cargo run -p wrfcoin-shared-protocol --example weather_payload_migration -- report <input.json|input.ndjson> [output.json]
  cargo run -p wrfcoin-shared-protocol --example weather_payload_migration -- canonicalize <input.json|input.ndjson> [output.json]"
    );
    process::exit(2);
}

fn read_input_records(input_path: &str) -> Result<Vec<InputRecord>, String> {
    let raw = fs::read_to_string(input_path)
        .map_err(|error| format!("failed reading '{input_path}': {error}"))?;

    let values = parse_input_values(&raw)
        .map_err(|error| format!("failed parsing '{input_path}' as JSON/NDJSON: {error}"))?;
    let records = values.into_iter().map(to_input_record).collect::<Vec<_>>();
    Ok(records)
}

fn parse_input_values(raw: &str) -> Result<Vec<Value>, String> {
    match serde_json::from_str::<Value>(raw) {
        Ok(Value::Array(values)) => Ok(values),
        Ok(value @ Value::Object(_)) => Ok(vec![value]),
        Ok(_) => Err("top-level JSON must be an object or array".to_string()),
        Err(_) => parse_ndjson_values(raw),
    }
}

fn parse_ndjson_values(raw: &str) -> Result<Vec<Value>, String> {
    let mut values = Vec::new();
    for (line_number, line) in raw.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let value = serde_json::from_str::<Value>(trimmed)
            .map_err(|error| format!("line {}: {error}", line_number + 1))?;
        values.push(value);
    }
    if values.is_empty() {
        return Err("no JSON records found".to_string());
    }
    Ok(values)
}

fn to_input_record(value: Value) -> InputRecord {
    let payload = value
        .as_object()
        .and_then(|object| {
            object
                .get("data")
                .or_else(|| object.get("payload"))
                .cloned()
                .filter(|candidate| candidate.is_object())
        })
        .unwrap_or_else(|| value.clone());

    let station_id = extract_text(&payload, &["station_id", "stationId"]).or_else(|| {
        extract_text(
            &value,
            &["station_id", "stationId", "device_id", "deviceId"],
        )
    });
    let client_id = extract_text(
        &value,
        &[
            "client_id",
            "clientId",
            "source",
            "provider",
            "device_id",
            "deviceId",
        ],
    );

    InputRecord {
        payload,
        station_id,
        client_id,
    }
}

fn extract_text(value: &Value, keys: &[&str]) -> Option<String> {
    let object = value.as_object()?;
    keys.iter().find_map(|key| {
        object
            .get(*key)
            .and_then(Value::as_str)
            .map(|raw| raw.to_string())
    })
}

fn build_report(input_file: &str, records: &[InputRecord]) -> MigrationReport {
    let mut classification = ClassificationCounts::default();
    let mut by_station: BTreeMap<String, ClassificationCounts> = BTreeMap::new();
    let mut by_client: BTreeMap<String, ClassificationCounts> = BTreeMap::new();

    for record in records {
        let record_classification =
            classify_weather_payload(&record.payload, WeatherPayloadParseOptions::default());
        classification.increment(record_classification);

        if let Some(station_id) = &record.station_id {
            by_station
                .entry(station_id.clone())
                .or_default()
                .increment(record_classification);
        }
        if let Some(client_id) = &record.client_id {
            by_client
                .entry(client_id.clone())
                .or_default()
                .increment(record_classification);
        }
    }

    let weather_total = classification.weather_total() as f64;
    let percentages = if weather_total > 0.0 {
        AdoptionPercentages {
            canonical_v1_of_weather: (classification.canonical_v1 as f64 / weather_total) * 100.0,
            legacy_unversioned_of_weather: (classification.legacy_unversioned as f64
                / weather_total)
                * 100.0,
            unsupported_or_invalid_of_weather: ((classification.unsupported_schema_version
                + classification.invalid_schema_version_marker_type
                + classification.invalid_canonical_payload)
                as f64
                / weather_total)
                * 100.0,
        }
    } else {
        AdoptionPercentages {
            canonical_v1_of_weather: 0.0,
            legacy_unversioned_of_weather: 0.0,
            unsupported_or_invalid_of_weather: 0.0,
        }
    };

    MigrationReport {
        input_file: input_file.to_string(),
        total_records: records.len() as u64,
        classification,
        adoption_percentages: percentages,
        by_station,
        by_client,
    }
}

fn canonicalize_records(records: &[InputRecord], options: WeatherPayloadParseOptions) -> Value {
    let mut output = Vec::with_capacity(records.len());

    for (index, record) in records.iter().enumerate() {
        let classification = classify_weather_payload(&record.payload, options);
        let canonical = canonicalize_weather_payload(&record.payload, options)
            .ok()
            .flatten();
        output.push(serde_json::json!({
            "index": index,
            "classification": classification,
            "station_id": record.station_id,
            "client_id": record.client_id,
            "canonical_payload": canonical,
        }));
    }

    Value::Array(output)
}

fn write_json_output<T: Serialize>(value: &T, output_path: Option<&str>) -> Result<(), String> {
    let rendered = serde_json::to_string_pretty(value)
        .map_err(|error| format!("serialization failed: {error}"))?;
    if let Some(path) = output_path {
        fs::write(path, format!("{rendered}\n"))
            .map_err(|error| format!("failed writing '{path}': {error}"))?;
    } else {
        println!("{rendered}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wrfcoin_shared_protocol::weather::schema::WEATHER_SCHEMA_V1;

    #[test]
    fn report_includes_deterministic_adoption_and_breakdowns() {
        let records = vec![
            InputRecord {
                payload: serde_json::json!({
                    "schema_version": WEATHER_SCHEMA_V1,
                    "observation": {"temperature_c": 20.0}
                }),
                station_id: Some("station-a".to_string()),
                client_id: Some("client-x".to_string()),
            },
            InputRecord {
                payload: serde_json::json!({
                    "schema_version": WEATHER_SCHEMA_V1,
                    "observation": {"humidity": 55.0}
                }),
                station_id: Some("station-b".to_string()),
                client_id: Some("client-x".to_string()),
            },
            InputRecord {
                payload: serde_json::json!({"temperature": 19.0}),
                station_id: Some("station-a".to_string()),
                client_id: Some("client-y".to_string()),
            },
            InputRecord {
                payload: serde_json::json!({
                    "schema_version": "wrf.weather.observation.v2",
                    "observation": {"temperature_c": 1.0}
                }),
                station_id: Some("station-c".to_string()),
                client_id: Some("client-x".to_string()),
            },
            InputRecord {
                payload: serde_json::json!({"foo": "bar"}),
                station_id: None,
                client_id: Some("client-y".to_string()),
            },
        ];

        let report = build_report("sample.ndjson", &records);
        assert_eq!(report.total_records, 5);
        assert_eq!(
            report.classification,
            ClassificationCounts {
                canonical_v1: 2,
                legacy_unversioned: 1,
                unsupported_schema_version: 1,
                invalid_schema_version_marker_type: 0,
                invalid_canonical_payload: 0,
                non_weather: 1,
            }
        );
        assert_eq!(
            report.adoption_percentages,
            AdoptionPercentages {
                canonical_v1_of_weather: 50.0,
                legacy_unversioned_of_weather: 25.0,
                unsupported_or_invalid_of_weather: 25.0,
            }
        );
        assert_eq!(
            report.by_station.get("station-a"),
            Some(&ClassificationCounts {
                canonical_v1: 1,
                legacy_unversioned: 1,
                unsupported_schema_version: 0,
                invalid_schema_version_marker_type: 0,
                invalid_canonical_payload: 0,
                non_weather: 0,
            })
        );
        assert_eq!(
            report.by_client.get("client-x"),
            Some(&ClassificationCounts {
                canonical_v1: 2,
                legacy_unversioned: 0,
                unsupported_schema_version: 1,
                invalid_schema_version_marker_type: 0,
                invalid_canonical_payload: 0,
                non_weather: 0,
            })
        );
    }

    #[test]
    fn canonicalize_records_keeps_classification_and_canonical_output_stable() {
        let records = vec![
            InputRecord {
                payload: serde_json::json!({
                    "stationId": "station-legacy",
                    "observedAtUnix": "1700111222",
                    "temperature": 21.5,
                    "humidity": 61.0
                }),
                station_id: Some("station-legacy".to_string()),
                client_id: Some("client-alpha".to_string()),
            },
            InputRecord {
                payload: serde_json::json!({
                    "schema_version": "wrf.weather.observation.v2",
                    "observation": {"temperature_c": 20.0}
                }),
                station_id: Some("station-unsupported".to_string()),
                client_id: Some("client-alpha".to_string()),
            },
        ];

        let canonicalized = canonicalize_records(&records, WeatherPayloadParseOptions::default());
        assert_eq!(
            canonicalized,
            serde_json::json!([
                {
                    "index": 0,
                    "classification": "LegacyUnversioned",
                    "station_id": "station-legacy",
                    "client_id": "client-alpha",
                    "canonical_payload": {
                        "schema_version": WEATHER_SCHEMA_V1,
                        "station_id": "station-legacy",
                        "observed_at_unix": 1700111222,
                        "observation": {
                            "temperature_c": 21.5,
                            "humidity": 61.0
                        }
                    }
                },
                {
                    "index": 1,
                    "classification": "UnsupportedSchemaVersion",
                    "station_id": "station-unsupported",
                    "client_id": "client-alpha",
                    "canonical_payload": serde_json::Value::Null
                }
            ])
        );
    }
}
