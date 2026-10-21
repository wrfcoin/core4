//! Bounded, byte-preserving metadata for future on-chain NFTs.
//!
//! The JSON text is the document: signatures and content hashes must bind
//! these exact UTF-8 bytes. Both JSON transport and bincode carry a STRING,
//! never an untagged `serde_json::Value`. No consensus path uses this type yet.

use std::collections::BTreeSet;
use std::fmt;

use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

pub const MAX_METADATA_BYTES: usize = 16 * 1024;
pub const MAX_METADATA_DEPTH: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MetadataError {
    #[error("NFT metadata exceeds 16384 UTF-8 bytes")]
    TooLarge,
    #[error("NFT metadata must be a JSON object")]
    NotObject,
    #[error("invalid NFT metadata: {0}")]
    InvalidJson(String),
}

/// Immutable JSON document with an explicit string encoding on every boundary.
///
/// Formatting and number spellings are retained intentionally. The document
/// hash describes bytes, not semantic equivalence between JSON objects.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct NftMetadataDocument(String);

impl NftMetadataDocument {
    pub fn parse(json: &str) -> Result<Self, MetadataError> {
        if json.len() > MAX_METADATA_BYTES {
            return Err(MetadataError::TooLarge);
        }
        let mut decoder = serde_json::Deserializer::from_str(json);
        let object = JsonShape { depth: 0 }
            .deserialize(&mut decoder)
            .map_err(|error| MetadataError::InvalidJson(error.to_string()))?;
        decoder
            .end()
            .map_err(|error| MetadataError::InvalidJson(error.to_string()))?;
        if !object {
            return Err(MetadataError::NotObject);
        }
        Ok(Self(json.to_owned()))
    }

    pub fn as_json_str(&self) -> &str {
        &self.0
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

impl<'de> Deserialize<'de> for NftMetadataDocument {
    fn deserialize<D: Deserializer<'de>>(decoder: D) -> Result<Self, D::Error> {
        struct DocumentVisitor;
        impl Visitor<'_> for DocumentVisitor {
            type Value = NftMetadataDocument;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a bounded JSON object encoded as a string")
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
                NftMetadataDocument::parse(value).map_err(E::custom)
            }
        }
        decoder.deserialize_str(DocumentVisitor)
    }
}

/// Validate shape without converting and re-emitting floating-point numbers.
struct JsonShape {
    depth: usize,
}

impl<'de> DeserializeSeed<'de> for JsonShape {
    type Value = bool;

    fn deserialize<D: Deserializer<'de>>(self, decoder: D) -> Result<bool, D::Error> {
        if self.depth > MAX_METADATA_DEPTH {
            return Err(de::Error::custom("metadata nesting exceeds 32 levels"));
        }
        decoder.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for JsonShape {
    type Value = bool;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("JSON metadata")
    }

    fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<bool, M::Error> {
        let mut keys = BTreeSet::new();
        while let Some(key) = map.next_key::<String>()? {
            if !keys.insert(key) {
                return Err(de::Error::custom("duplicate metadata member name"));
            }
            map.next_value_seed(JsonShape {
                depth: self.depth + 1,
            })?;
        }
        Ok(true)
    }

    fn visit_seq<S: SeqAccess<'de>>(self, mut sequence: S) -> Result<bool, S::Error> {
        while sequence
            .next_element_seed(JsonShape {
                depth: self.depth + 1,
            })?
            .is_some()
        {}
        Ok(false)
    }

    fn visit_bool<E: de::Error>(self, _: bool) -> Result<bool, E> {
        Ok(false)
    }

    fn visit_i64<E: de::Error>(self, _: i64) -> Result<bool, E> {
        Ok(false)
    }

    fn visit_u64<E: de::Error>(self, _: u64) -> Result<bool, E> {
        Ok(false)
    }

    // determinism-ok: JSON shape validation discards this value; the original UTF-8 document bytes are retained without floating arithmetic or re-encoding.
    fn visit_f64<E: de::Error>(self, _: f64) -> Result<bool, E> {
        Ok(false)
    }

    fn visit_str<E: de::Error>(self, _: &str) -> Result<bool, E> {
        Ok(false)
    }

    fn visit_unit<E: de::Error>(self) -> Result<bool, E> {
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn populated_document_retains_exact_bytes_across_binary_and_json() {
        let text = r#" { "z": [1, 2.500, null, true], "a": {"n": -0.0, "s": "雪"} } "#;
        let mut document = NftMetadataDocument::parse(text).unwrap();
        for _ in 0..5 {
            let binary = bincode::serialize(&document).unwrap();
            let restored: NftMetadataDocument = bincode::deserialize(&binary).unwrap();
            assert_eq!(restored.as_bytes(), text.as_bytes());
            let transport = serde_json::to_string(&restored).unwrap();
            let from_json: NftMetadataDocument = serde_json::from_str(&transport).unwrap();
            assert_eq!(from_json, document);
            document = from_json;
        }
    }

    #[test]
    fn binary_golden_is_length_followed_by_document_bytes() {
        let document = NftMetadataDocument::parse(r#"{"a":1}"#).unwrap();
        assert_eq!(
            bincode::serialize(&document).unwrap(),
            [
                7, 0, 0, 0, 0, 0, 0, 0, b'{', b'"', b'a', b'"', b':', b'1', b'}'
            ]
        );
    }

    #[test]
    fn historical_value_encoding_does_not_round_trip() {
        let value = serde_json::json!({"output": [1, "result"]});
        let bytes = bincode::serialize(&value).unwrap();
        assert!(matches!(
            *bincode::deserialize::<serde_json::Value>(&bytes).unwrap_err(),
            bincode::ErrorKind::DeserializeAnyNotSupported
        ));
    }

    #[test]
    fn malformed_nonobject_and_duplicate_names_are_rejected() {
        for text in [
            "",
            "[]",
            "null",
            "1",
            r#"{"a":NaN}"#,
            r#"{} trailing"#,
            r#"{"a":1,"a":2}"#,
            r#"{"a":{"x":1,"\u0078":2}}"#,
        ] {
            assert!(NftMetadataDocument::parse(text).is_err(), "{text}");
        }
        assert!(NftMetadataDocument::parse(r#"{"a":{},"b":{"a":1}}"#).is_ok());
    }

    #[test]
    fn byte_limit_is_inclusive_and_measures_utf8_bytes() {
        let mut text = "{}".to_string();
        text.push_str(&" ".repeat(MAX_METADATA_BYTES - text.len()));
        assert!(NftMetadataDocument::parse(&text).is_ok());
        text.push(' ');
        assert_eq!(
            NftMetadataDocument::parse(&text),
            Err(MetadataError::TooLarge)
        );
        let unicode = format!(r#"{{"x":"{}"}}"#, "雪".repeat(MAX_METADATA_BYTES / 2));
        assert_eq!(
            NftMetadataDocument::parse(&unicode),
            Err(MetadataError::TooLarge)
        );
    }

    #[test]
    fn nesting_limit_is_inclusive() {
        let nested =
            |count: usize| format!(r#"{{"x":{}0{}}}"#, "[".repeat(count), "]".repeat(count));
        assert!(NftMetadataDocument::parse(&nested(MAX_METADATA_DEPTH - 1)).is_ok());
        assert!(NftMetadataDocument::parse(&nested(MAX_METADATA_DEPTH)).is_err());
    }

    #[test]
    fn decoding_revalidates_documents_in_both_formats() {
        for bad in [
            "[]".to_string(),
            "{".to_string(),
            " ".repeat(MAX_METADATA_BYTES + 1),
        ] {
            let binary = bincode::serialize(&bad).unwrap();
            assert!(bincode::deserialize::<NftMetadataDocument>(&binary).is_err());
            let json = serde_json::to_string(&bad).unwrap();
            assert!(serde_json::from_str::<NftMetadataDocument>(&json).is_err());
        }
        assert!(serde_json::from_str::<NftMetadataDocument>(r#"{"a":1}"#).is_err());
    }
}
