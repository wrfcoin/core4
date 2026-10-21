//! The operator <-> phone recovery exchange text (#3345 Q1, spec §13).
//!
//! Self-describing, versioned, checksummed text that travels as a QR or a
//! paste: `WRFOWN1.<kind>.<base64url-nopad(json)>.<checksum8>`, where
//! `checksum8` is the first 8 hex characters of SHA-256 over everything before
//! it (`WRFOWN1.<kind>.<b64>`). It carries ONLY public values. Decoding is
//! strict: whitespace is stripped, the version must be 1, the key set must be
//! exact, and hex fields must be lowercase hex of the right width.
//!
//! This is the SAME framing as the phone's `station_owner_exchange.dart`
//! (mobile#414): the JSON is emitted with keys in the phone's insertion order,
//! compact, so the bytes (and the checksum) match a phone-built text exactly.
//! The shared test vector is in the tests below and in the spec.

use anyhow::{Result, anyhow};
use base64::Engine as _;
use base64::engine::general_purpose::{URL_SAFE, URL_SAFE_NO_PAD};
use serde_json::Value;
use sha2::{Digest, Sha256};

pub const EXCHANGE_PREFIX: &str = "WRFOWN1";
pub const RECOVERY_REQUEST_KIND: &str = "station_recovery_request_v1";
pub const RECOVERY_ACCEPT_KIND: &str = "station_recovery_accept_v1";
/// The framing `kind` segment for each text.
pub const FRAME_RECOVERY: &str = "recovery";
pub const FRAME_RECOVERY_ACCEPT: &str = "recovery-accept";

/// The public recovery body both texts carry (the same fields the filing and
/// acceptance preimages commit to; core4 `station_owner_recovery_message_with_domain`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryBody {
    pub chain_id: String,
    pub network_id: String,
    pub station_id: String,
    pub from_owner_address: String,
    pub to_owner_address: String,
    pub to_owner_pubkey: String,
    pub recovery_nonce: u64,
}

fn checksum(body: &str) -> String {
    hex::encode(Sha256::digest(body.as_bytes()))[..8].to_string()
}

fn q(s: &str) -> String {
    // One JSON string literal; ASCII inputs encode identically to Dart's jsonEncode.
    serde_json::to_string(s).expect("a string always serialises")
}

/// The body fields as compact JSON in the phone's order (no surrounding braces).
fn body_fields(b: &RecoveryBody) -> String {
    format!(
        "\"chain_id\":{},\"network_id\":{},\"station_id\":{},\"from_owner_address\":{},\
         \"to_owner_address\":{},\"to_owner_pubkey\":{},\"recovery_nonce\":{}",
        q(&b.chain_id),
        q(&b.network_id),
        q(&b.station_id),
        q(&b.from_owner_address),
        q(&b.to_owner_address),
        q(&b.to_owner_pubkey),
        b.recovery_nonce
    )
}

fn pack(frame_kind: &str, json: &str) -> String {
    let b64 = URL_SAFE_NO_PAD.encode(json.as_bytes());
    let body = format!("{EXCHANGE_PREFIX}.{frame_kind}.{b64}");
    format!("{body}.{}", checksum(&body))
}

/// The operator's request: `WRFOWN1.recovery.<...>`.
pub fn encode_recovery_request(b: &RecoveryBody) -> String {
    pack(
        FRAME_RECOVERY,
        &format!(
            "{{\"v\":1,\"kind\":{},{}}}",
            q(RECOVERY_REQUEST_KIND),
            body_fields(b)
        ),
    )
}

/// The claimant's reply: `WRFOWN1.recovery-accept.<...>`.
pub fn encode_recovery_accept(b: &RecoveryBody, to_owner_signature: &str) -> String {
    pack(
        FRAME_RECOVERY_ACCEPT,
        &format!(
            "{{\"v\":1,\"kind\":{},{},\"to_owner_signature\":{}}}",
            q(RECOVERY_ACCEPT_KIND),
            body_fields(b),
            q(to_owner_signature)
        ),
    )
}

fn lower_hex(v: &str, len: usize) -> bool {
    v.len() == len && v.bytes().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f'))
}

fn object(text: &str, frame_kind: &str, keys: &[&str]) -> Result<serde_json::Map<String, Value>> {
    let t: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    let parts: Vec<&str> = t.split('.').collect();
    if parts.len() != 4 || parts[0] != EXCHANGE_PREFIX {
        return Err(anyhow!("not an owner exchange code"));
    }
    if parts[1] != frame_kind {
        return Err(anyhow!("wrong kind of code"));
    }
    let body = format!("{}.{}.{}", parts[0], parts[1], parts[2]);
    if parts[3] != checksum(&body) {
        return Err(anyhow!("code is truncated or damaged"));
    }
    let raw = URL_SAFE_NO_PAD
        .decode(parts[2].trim_end_matches('='))
        .or_else(|_| URL_SAFE.decode(parts[2]))
        .map_err(|_| anyhow!("not a valid exchange code"))?;
    let value: Value = serde_json::from_slice(&raw).map_err(|_| anyhow!("not a valid exchange code"))?;
    let Value::Object(map) = value else {
        return Err(anyhow!("not an exchange object"));
    };
    if map.get("v") != Some(&Value::from(1)) {
        return Err(anyhow!("unsupported exchange version"));
    }
    let have: std::collections::BTreeSet<&str> = map.keys().map(String::as_str).collect();
    let want: std::collections::BTreeSet<&str> = keys.iter().copied().collect();
    if have != want {
        return Err(anyhow!("unexpected exchange fields"));
    }
    Ok(map)
}

fn field(m: &serde_json::Map<String, Value>, k: &str) -> Result<String> {
    match m.get(k) {
        Some(Value::String(s)) if !s.is_empty() => Ok(s.clone()),
        _ => Err(anyhow!("bad field {k}")),
    }
}

fn body_of(m: &serde_json::Map<String, Value>) -> Result<RecoveryBody> {
    let to_owner_pubkey = field(m, "to_owner_pubkey")?;
    if !lower_hex(&to_owner_pubkey, 64) {
        return Err(anyhow!("bad field to_owner_pubkey"));
    }
    let recovery_nonce = m
        .get("recovery_nonce")
        .and_then(Value::as_u64)
        .ok_or_else(|| anyhow!("bad field recovery_nonce"))?;
    Ok(RecoveryBody {
        chain_id: field(m, "chain_id")?,
        network_id: field(m, "network_id")?,
        station_id: field(m, "station_id")?,
        from_owner_address: field(m, "from_owner_address")?,
        to_owner_address: field(m, "to_owner_address")?,
        to_owner_pubkey,
        recovery_nonce,
    })
}

const BODY_KEYS: [&str; 9] = [
    "v",
    "kind",
    "chain_id",
    "network_id",
    "station_id",
    "from_owner_address",
    "to_owner_address",
    "to_owner_pubkey",
    "recovery_nonce",
];

/// Decode an operator request (`WRFOWN1.recovery.…`).
pub fn decode_recovery_request(text: &str) -> Result<RecoveryBody> {
    let m = object(text, FRAME_RECOVERY, &BODY_KEYS)?;
    if m.get("kind").and_then(Value::as_str) != Some(RECOVERY_REQUEST_KIND) {
        return Err(anyhow!("wrong kind"));
    }
    body_of(&m)
}

/// Decode the claimant's reply (`WRFOWN1.recovery-accept.…`): the body plus the
/// countersignature (`to_owner_signature`, 128 lowercase hex).
pub fn decode_recovery_accept(text: &str) -> Result<(RecoveryBody, String)> {
    let mut keys = BODY_KEYS.to_vec();
    keys.push("to_owner_signature");
    let m = object(text, FRAME_RECOVERY_ACCEPT, &keys)?;
    if m.get("kind").and_then(Value::as_str) != Some(RECOVERY_ACCEPT_KIND) {
        return Err(anyhow!("wrong kind"));
    }
    let signature = field(&m, "to_owner_signature")?;
    if !lower_hex(&signature, 128) {
        return Err(anyhow!("bad field to_owner_signature"));
    }
    Ok((body_of(&m)?, signature))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The SHARED test vector (spec §13): the phone's `station_owner_exchange.dart`
    /// must produce and accept exactly these texts for these inputs.
    pub(crate) fn vector_body() -> RecoveryBody {
        RecoveryBody {
            chain_id: "wrfcoin-testnet".to_string(),
            network_id: "testnet".to_string(),
            station_id: "station-golden-3345".to_string(),
            from_owner_address: format!("wrf{}", "a1".repeat(20)),
            to_owner_address: format!("wrf{}", "c3".repeat(20)),
            to_owner_pubkey: "c4".repeat(32),
            recovery_nonce: 7,
        }
    }
    pub(crate) fn vector_signature() -> String {
        "ab".repeat(64)
    }

    #[test]
    fn request_and_accept_round_trip_and_match_the_pinned_vector() {
        let body = vector_body();
        let request = encode_recovery_request(&body);
        let accept = encode_recovery_accept(&body, &vector_signature());
        assert_eq!(request, VECTOR_REQUEST, "request text drifted from the pinned vector");
        assert_eq!(accept, VECTOR_ACCEPT, "accept text drifted from the pinned vector");
        assert_eq!(decode_recovery_request(&request).unwrap(), body);
        assert_eq!(
            decode_recovery_accept(&accept).unwrap(),
            (body, vector_signature())
        );
    }

    #[test]
    fn decoding_is_strict() {
        let body = vector_body();
        let request = encode_recovery_request(&body);
        // Whitespace and wrapping are tolerated.
        let wrapped = format!("  {}\n{}  ", &request[..20], &request[20..]);
        assert_eq!(decode_recovery_request(&wrapped).unwrap(), body);
        // Wrong frame kind, wrong body kind, damaged checksum, truncation.
        assert!(decode_recovery_accept(&request).is_err());
        assert!(decode_recovery_request(&request.replacen(".recovery.", ".offer.", 1)).is_err());
        let mut damaged = request.clone();
        damaged.pop();
        damaged.push(if request.ends_with('0') { '1' } else { '0' });
        assert!(decode_recovery_request(&damaged).is_err());
        assert!(decode_recovery_request(&request[..request.len() - 12]).is_err());
        // A request is not an accept and vice versa, even re-framed with a valid checksum.
        let accept = encode_recovery_accept(&body, &vector_signature());
        assert!(decode_recovery_request(&accept).is_err());
        // Extra fields are refused (key material can never ride along).
        let extra = pack(
            FRAME_RECOVERY,
            &format!(
                "{{\"v\":1,\"kind\":\"{RECOVERY_REQUEST_KIND}\",{},\"secret\":\"x\"}}",
                body_fields(&body)
            ),
        );
        assert!(decode_recovery_request(&extra).is_err());
        // Uppercase or short hex is refused.
        let mut bad = body.clone();
        bad.to_owner_pubkey = "C4".repeat(32);
        assert!(decode_recovery_request(&encode_recovery_request(&bad)).is_err());
        assert!(decode_recovery_accept(&encode_recovery_accept(&body, "ab")).is_err());
    }

    const VECTOR_REQUEST: &str =
        "WRFOWN1.recovery.eyJ2IjoxLCJraW5kIjoic3RhdGlvbl9yZWNvdmVyeV9yZXF1ZXN0X3YxIiwiY2hhaW5faWQiOiJ3cmZjb2luLXRlc3RuZXQiLCJuZXR3b3JrX2lkIjoidGVzdG5ldCIsInN0YXRpb25faWQiOiJzdGF0aW9uLWdvbGRlbi0zMzQ1IiwiZnJvbV9vd25lcl9hZGRyZXNzIjoid3JmYTFhMWExYTFhMWExYTFhMWExYTFhMWExYTFhMWExYTFhMWExYTFhMSIsInRvX293bmVyX2FkZHJlc3MiOiJ3cmZjM2MzYzNjM2MzYzNjM2MzYzNjM2MzYzNjM2MzYzNjM2MzYzNjM2MzIiwidG9fb3duZXJfcHVia2V5IjoiYzRjNGM0YzRjNGM0YzRjNGM0YzRjNGM0YzRjNGM0YzRjNGM0YzRjNGM0YzRjNGM0YzRjNGM0YzRjNGM0YzRjNCIsInJlY292ZXJ5X25vbmNlIjo3fQ.5278cf99";
    const VECTOR_ACCEPT: &str =
        "WRFOWN1.recovery-accept.eyJ2IjoxLCJraW5kIjoic3RhdGlvbl9yZWNvdmVyeV9hY2NlcHRfdjEiLCJjaGFpbl9pZCI6IndyZmNvaW4tdGVzdG5ldCIsIm5ldHdvcmtfaWQiOiJ0ZXN0bmV0Iiwic3RhdGlvbl9pZCI6InN0YXRpb24tZ29sZGVuLTMzNDUiLCJmcm9tX293bmVyX2FkZHJlc3MiOiJ3cmZhMWExYTFhMWExYTFhMWExYTFhMWExYTFhMWExYTFhMWExYTFhMWExIiwidG9fb3duZXJfYWRkcmVzcyI6IndyZmMzYzNjM2MzYzNjM2MzYzNjM2MzYzNjM2MzYzNjM2MzYzNjM2MzYzMiLCJ0b19vd25lcl9wdWJrZXkiOiJjNGM0YzRjNGM0YzRjNGM0YzRjNGM0YzRjNGM0YzRjNGM0YzRjNGM0YzRjNGM0YzRjNGM0YzRjNGM0YzRjNGM0IiwicmVjb3Zlcnlfbm9uY2UiOjcsInRvX293bmVyX3NpZ25hdHVyZSI6ImFiYWJhYmFiYWJhYmFiYWJhYmFiYWJhYmFiYWJhYmFiYWJhYmFiYWJhYmFiYWJhYmFiYWJhYmFiYWJhYmFiYWJhYmFiYWJhYmFiYWJhYmFiYWJhYmFiYWJhYmFiYWJhYmFiYWJhYmFiYWJhYmFiYWJhYmFiYWJhYmFiYWJhYmFiIn0.2292a693";
}
