use super::*;

fn radio() -> RadioSourceMetadata {
    RadioSourceMetadata {
        source_type: RadioSourceType::Sdr,
        frequency_mhz: Some(7.0),
        signal_strength_dbm: Some(8.0),
        protocol: Some("synthetic".into()),
        raw_packet_hash: None,
        device_id: None,
    }
}

#[test]
fn canonical_observation_preserves_wire_layout() {
    let value = WeatherObservation {
        temperature_c: Some(1.0),
        humidity_percent: Some(2.0),
        pressure_hpa: Some(3.0),
        wind_speed_ms: Some(4.0),
        precipitation_mm: Some(5.0),
        dewpoint_c: Some(6.0),
        radio_metadata: Some(radio()),
    };
    let bytes: &[u8] = &[
        1, 0, 0, 0, 0, 0, 0, 240, 63, 1, 0, 0, 0, 0, 0, 0, 0, 64, 1, 0, 0, 0, 0, 0, 0, 8, 64, 1, 0,
        0, 0, 0, 0, 0, 16, 64, 1, 0, 0, 0, 0, 0, 0, 20, 64, 1, 0, 0, 0, 0, 0, 0, 24, 64, 1, 0, 0,
        0, 0, 1, 0, 0, 0, 0, 0, 0, 28, 64, 1, 0, 0, 0, 0, 0, 0, 32, 64, 1, 9, 0, 0, 0, 0, 0, 0, 0,
        115, 121, 110, 116, 104, 101, 116, 105, 99, 0, 0,
    ];
    assert_eq!(bincode::serialize(&value).unwrap(), bytes);
    assert_eq!(
        bincode::deserialize::<WeatherObservation>(bytes).unwrap(),
        value
    );
    assert_eq!(
        serde_json::to_value(&value).unwrap(),
        serde_json::json!({
            "temperature_c":1.0,"humidity_percent":2.0,"pressure_hpa":3.0,"wind_speed_ms":4.0,
            "precipitation_mm":5.0,"dewpoint_c":6.0,"radio_metadata":{
                "source_type":"Sdr","frequency_mhz":7.0,"signal_strength_dbm":8.0,
                "protocol":"synthetic","raw_packet_hash":null,"device_id":null
            }
        })
    );
}

#[test]
fn environmental_report_preserves_wider_wire_layout() {
    let value = WeatherPayload {
        temperature_c: Some(1.0),
        humidity_percent: Some(2.0),
        pressure_hpa: Some(3.0),
        wind_speed_ms: Some(4.0),
        wind_direction_deg: Some(40.0),
        precipitation_mm: Some(5.0),
        dewpoint_c: Some(6.0),
        visibility_m: Some(80.0),
        radio_metadata: Some(radio()),
    };
    let bytes: &[u8] = &[
        1, 0, 0, 0, 0, 0, 0, 240, 63, 1, 0, 0, 0, 0, 0, 0, 0, 64, 1, 0, 0, 0, 0, 0, 0, 8, 64, 1, 0,
        0, 0, 0, 0, 0, 16, 64, 1, 0, 0, 0, 0, 0, 0, 68, 64, 1, 0, 0, 0, 0, 0, 0, 20, 64, 1, 0, 0,
        0, 0, 0, 0, 24, 64, 1, 0, 0, 0, 0, 0, 0, 84, 64, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 28,
        64, 1, 0, 0, 0, 0, 0, 0, 32, 64, 1, 9, 0, 0, 0, 0, 0, 0, 0, 115, 121, 110, 116, 104, 101,
        116, 105, 99, 0, 0,
    ];
    assert_eq!(bincode::serialize(&value).unwrap(), bytes);
    assert_eq!(
        bincode::deserialize::<WeatherPayload>(bytes).unwrap(),
        value
    );
    let report = ReportPayload::Weather(value.clone());
    let mut tagged_bytes = vec![0, 0, 0, 0];
    tagged_bytes.extend_from_slice(bytes);
    assert_eq!(bincode::serialize(&report).unwrap(), tagged_bytes);
    assert_eq!(
        serde_json::to_value(&report).unwrap(),
        serde_json::json!({
            "domain":"Weather","data":{
                "temperature_c":1.0,"humidity_percent":2.0,"pressure_hpa":3.0,"wind_speed_ms":4.0,
                "wind_direction_deg":40.0,"precipitation_mm":5.0,"dewpoint_c":6.0,"visibility_m":80.0,
                "radio_metadata":{"source_type":"Sdr","frequency_mhz":7.0,"signal_strength_dbm":8.0,
                    "protocol":"synthetic","raw_packet_hash":null,"device_id":null}
            }
        })
    );
}

#[cfg(feature = "ts")]
#[test]
fn canonical_typescript_matches_node_shape() {
    use ts_rs::TS;
    let declaration = WeatherObservation::decl();
    for field in [
        "temperature_c",
        "humidity_percent",
        "pressure_hpa",
        "wind_speed_ms",
        "precipitation_mm",
        "dewpoint_c",
    ] {
        assert!(
            declaration.contains(&format!("{field}: number | null")),
            "{declaration}"
        );
    }
    assert!(
        declaration.contains("radio_metadata?: RadioSourceMetadata | null"),
        "{declaration}"
    );
    assert!(!declaration.contains("wind_direction_deg"), "{declaration}");
    assert!(!declaration.contains("visibility_m"), "{declaration}");
}

#[cfg(feature = "ts")]
#[test]
fn environmental_typescript_retains_wider_report_shape() {
    use ts_rs::TS;
    let declaration = EnvironmentalWeatherMeasurements::decl();
    assert!(
        declaration.contains("wind_direction_deg: number | null"),
        "{declaration}"
    );
    assert!(
        declaration.contains("visibility_m: number | null"),
        "{declaration}"
    );
    let report = ReportPayload::decl();
    assert!(
        report.contains("\"data\": EnvironmentalWeatherMeasurements"),
        "{report}"
    );
    assert!(!report.contains("\"data\": WeatherObservation"), "{report}");
}
