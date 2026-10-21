//! Cross-language commit-hash parity test for backend#767.
//!
//! The backend (`backend/shared/commit-reveal-hash.ts`) computes the weather
//! commit id in TypeScript so it can commit before revealing. core4 re-derives
//! it at reveal time via `Transaction::compute_commit_id`. If the two ever
//! diverge, reveals are rejected on-chain (the original #767 bug).
//!
//! These golden vectors are produced by the backend's TS implementation
//! (node `crypto.createHash('sha3-256')` over the canonical, sorted-key JSON
//! object). This test parses the exact JSON bytes the backend sends over the
//! wire and asserts core4 hashes them to the same hex — proving the SHA3-256
//! algorithm, the 5-field `||` preimage, sorted-key serialization, and V8↔ryu
//! float formatting all agree. Keep these vectors identical to the ones in
//! `backend/__tests__/commit-reveal-hash-parity.test.ts`.

use wrfcoin_native_chain::blockchain::transaction::Transaction;

/// (label, canonical data JSON as sent on the wire, nonce, station_id, timestamp, expected sha3 hex)
const VECTORS: &[(&str, &str, &str, &str, i64, &str)] = &[
    (
        "integer+fractional",
        r#"{"humidity_pct":65,"pressure_hpa":1013.25,"rainfall_mm":0,"temperature_c":22.5,"wind_mps":0}"#,
        "reveal-nonce",
        "WX-REVEAL-001",
        1_700_000_000,
        "f662f1e6a3e3c2cfb07ded9a65a9da0106250efe8985144ece9bf15bd8677708",
    ),
    (
        "all-fractional",
        r#"{"humidity_pct":54.2,"pressure_hpa":1009.7,"rainfall_mm":0.4,"temperature_c":18.3,"wind_mps":3.6}"#,
        "abc-123",
        "station-001",
        1_733_000_000,
        "140ded74a31cb1fb52d42a4c88b82764f1f05bb7e19b4aedd9d67cf08f9e7ee4",
    ),
    (
        "collector-style-converted",
        r#"{"humidity_pct":50,"pressure_hpa":1016.916,"rainfall_mm":0,"temperature_c":21.11111111111111,"wind_mps":2.2352}"#,
        "n1",
        "WX-DAVIS-7",
        1_699_999_999,
        "42bcec3eb1fe024428c10468723fb76b37a78d4082a3fdd8f72f21e6f14b2729",
    ),
    (
        "all-zero",
        r#"{"humidity_pct":0,"pressure_hpa":0,"rainfall_mm":0,"temperature_c":0,"wind_mps":0}"#,
        "z",
        "S0",
        1,
        "98db71e92de2b1327bba34822c2595347c820ad522a8adf6a0fec30ba42f9cd8",
    ),
];

#[test]
fn commit_id_matches_backend_golden_vectors() {
    for (label, data_json, nonce, station_id, timestamp, expected) in VECTORS {
        let data: serde_json::Value =
            serde_json::from_str(data_json).expect("vector data must be valid JSON");
        let got = Transaction::compute_commit_id(&data, nonce, station_id, *timestamp, "contribution");
        assert_eq!(
            &got, expected,
            "commit-id parity mismatch for vector '{label}': core4 produced {got}, backend expected {expected}"
        );
    }
}

/// #3285: collector-produced vectors (`tests/security/lib/ambient-pipeline.cjs`
/// `computeCommitId`, node `sha3-256` over the sorted-key reveal `data`). The
/// first carries the additive `pressure_sea_level_hpa` and `pressure_kind`
/// keys; the second is the same reading without them. core4 must re-derive both ids, and they must
/// differ: the extra key is covered by the commit id like every other key, so
/// it cannot be added or altered after commit. Keep these identical to the
/// twin in `tests/security/fetch-ambient-weather.test.js`.
const COLLECTOR_VECTORS: &[(&str, &str, &str, &str, i64, &str)] = &[
    (
        "sea-level-key-3285",
        r#"{"elevation_m":227.7,"humidity_pct":97,"precipitation_mm":0,"pressure_hpa":1001.2,"pressure_kind":"station","pressure_sea_level_hpa":1028.29,"temperature_c":10.2,"wind_speed_ms":0.4}"#,
        "n-3285",
        "PWS-EXAMPLE-WS5000",
        1_790_224_800,
        "7ce5f51074a3ee1959bfa5960ae72fae11d5129fd052a992a21280c4da7d10e3", // pragma: allowlist secret — a SHA3-256 commit id test vector, not a credential
    ),
    (
        "same-reading-without-sea-level-keys-3285",
        r#"{"elevation_m":227.7,"humidity_pct":97,"precipitation_mm":0,"pressure_hpa":1001.2,"temperature_c":10.2,"wind_speed_ms":0.4}"#,
        "n-3285",
        "PWS-EXAMPLE-WS5000",
        1_790_224_800,
        "617a12bdd305de8777bab29b3458041ffeee598876cbca2f5d6fe9591a01e5c9", // pragma: allowlist secret — a SHA3-256 commit id test vector, not a credential
    ),
];

#[test]
fn commit_id_covers_the_sea_level_pressure_key_3285() {
    for (label, data_json, nonce, station_id, timestamp, expected) in COLLECTOR_VECTORS {
        let data: serde_json::Value =
            serde_json::from_str(data_json).expect("vector data must be valid JSON");
        let got = Transaction::compute_commit_id(&data, nonce, station_id, *timestamp, "contribution");
        assert_eq!(
            &got, expected,
            "commit-id parity mismatch for collector vector '{label}': core4 produced {got}, collector expected {expected}"
        );
    }
    assert_ne!(COLLECTOR_VECTORS[0].5, COLLECTOR_VECTORS[1].5);
}
