#![no_main]

//! Fuzz target for `validate_physics()` in `shared-protocol/src/weather/validation.rs`.
//!
//! Exercises all `f64` fields (temperature, pressure, humidity, wind speed) including
//! NaN / Inf / subnormal values, rate-of-change with a previous reading, altitude-aware
//! pressure bounds, and every branch of `PhysicsRangePolicy` enforcement.
//!
//! The target asserts only that the function **never panics** on any input; the
//! returned `Vec<PhysicsViolation>` is intentionally discarded.
//!
//! # Input layout (minimum 121 bytes for a single reading)
//!
//! ```text
//! [0]       flags — bit 0: temperature Some, bit 1: pressure Some,
//!                   bit 2: humidity Some, bit 3: wind_speed Some,
//!                   bit 4: elevation Some, bit 5: prev_reading present
//! [1..9]    temperature_c       (LE f64)
//! [9..17]   pressure_hpa        (LE f64)
//! [17..25]  humidity_pct        (LE f64)
//! [25..33]  wind_speed_ms       (LE f64)
//! [33..41]  timestamp_unix      (LE i64)
//! [41..49]  policy.temp_min_c   (LE f64)
//! [49..57]  policy.temp_max_c   (LE f64)
//! [57..65]  policy.pressure_min_hpa (LE f64)
//! [65..73]  policy.pressure_max_hpa (LE f64)
//! [73..81]  policy.humidity_min_pct (LE f64)
//! [81..89]  policy.humidity_max_pct (LE f64)
//! [89..97]  policy.wind_speed_min_ms (LE f64)
//! [97..105] policy.wind_speed_max_ms (LE f64)
//! [105..113] policy.temp_rate_max_c_per_min (LE f64)
//! [113..121] policy.activation_height (LE u64)
//! [121..129] current_height (LE u64)
//! [129..137] elevation_m (LE f64, only used when flag bit 4 is set)
//!
//! If data.len() >= 179, bytes [137..178] encode prev_reading (same layout as
//! reading, minus policy/height) when flag bit 5 is set:
//!   [137]     prev_flags  (bits 0-3 as above)
//!   [138..146] prev.temperature_c
//!   [146..154] prev.pressure_hpa
//!   [154..162] prev.humidity_pct
//!   [162..170] prev.wind_speed_ms
//!   [170..178] prev.timestamp_unix
//! ```

use libfuzzer_sys::fuzz_target;
use wrfcoin_shared_protocol::weather::validation::{
    PhysicsRangePolicy, WeatherReading, validate_physics,
};

fn read_f64(data: &[u8], offset: usize) -> f64 {
    let mut buf = [0u8; 8];
    buf.copy_from_slice(&data[offset..offset + 8]);
    f64::from_le_bytes(buf)
}

fn read_i64(data: &[u8], offset: usize) -> i64 {
    let mut buf = [0u8; 8];
    buf.copy_from_slice(&data[offset..offset + 8]);
    i64::from_le_bytes(buf)
}

fn read_u64(data: &[u8], offset: usize) -> u64 {
    let mut buf = [0u8; 8];
    buf.copy_from_slice(&data[offset..offset + 8]);
    u64::from_le_bytes(buf)
}

fuzz_target!(|data: &[u8]| {
    // Minimum: flags(1) + 4×f64 fields(32) + timestamp(8) + 9×policy f64s(72) + activation(8) + current_height(8) = 129
    if data.len() < 129 {
        return;
    }

    let flags = data[0];

    let temperature_c = if flags & 0x01 != 0 { Some(read_f64(data, 1)) } else { None };
    let pressure_hpa = if flags & 0x02 != 0 { Some(read_f64(data, 9)) } else { None };
    let humidity_pct = if flags & 0x04 != 0 { Some(read_f64(data, 17)) } else { None };
    let wind_speed_ms = if flags & 0x08 != 0 { Some(read_f64(data, 25)) } else { None };
    let timestamp_unix = read_i64(data, 33);

    let reading = WeatherReading {
        temperature_c,
        pressure_hpa,
        humidity_pct,
        wind_speed_ms,
        timestamp_unix,
    };

    let policy = PhysicsRangePolicy {
        temp_min_c: read_f64(data, 41),
        temp_max_c: read_f64(data, 49),
        pressure_min_hpa: read_f64(data, 57),
        pressure_max_hpa: read_f64(data, 65),
        humidity_min_pct: read_f64(data, 73),
        humidity_max_pct: read_f64(data, 81),
        wind_speed_min_ms: read_f64(data, 89),
        wind_speed_max_ms: read_f64(data, 97),
        temp_rate_max_c_per_min: read_f64(data, 105),
        activation_height: read_u64(data, 113),
    };

    let current_height = read_u64(data, 121);

    // elevation_m: optional, requires byte [128] flag and bytes [129..137]
    let elevation_m = if flags & 0x10 != 0 && data.len() >= 137 {
        Some(read_f64(data, 129))
    } else {
        None
    };

    // prev_reading: optional, requires flag bit 5 and at least 178 bytes
    let prev_reading_opt: Option<WeatherReading> = if flags & 0x20 != 0 && data.len() >= 178 {
        let pf = data[137];
        Some(WeatherReading {
            temperature_c: if pf & 0x01 != 0 { Some(read_f64(data, 138)) } else { None },
            pressure_hpa: if pf & 0x02 != 0 { Some(read_f64(data, 146)) } else { None },
            humidity_pct: if pf & 0x04 != 0 { Some(read_f64(data, 154)) } else { None },
            wind_speed_ms: if pf & 0x08 != 0 { Some(read_f64(data, 162)) } else { None },
            timestamp_unix: read_i64(data, 170),
        })
    } else {
        None
    };

    // The only assertion: must not panic.
    let _ = validate_physics(
        &reading,
        prev_reading_opt.as_ref(),
        &policy,
        current_height,
        elevation_m,
    );
});
