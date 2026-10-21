//! Whole-`State` snapshot codec, format v2 (#3288 fix 3a).
//!
//! Format v1 hand-encoded a dozen `State` sections as pipe-delimited text and
//! silently left every other section, 27 of the 40 state-root sections among
//! them, at the importing node's own values (#3288). A node synced that way
//! could match the chain's root at the snapshot height and still diverge at
//! the next pruning or settlement, because block application reads state that
//! is not root material.
//!
//! v2 carries the WHOLE serialized `State`, driven by serde rather than by a
//! hand-maintained field list:
//!
//! - `meta/format_version` = `2`.
//! - One entry per top-level serialized field, `state/<field>`. A field whose
//!   value is a map (a map-typed field or a struct) is split one level into
//!   per-item entries `state/<field>/<hex(canonical key)>`, so large maps get
//!   granular inclusion proofs and paging; its `state/<field>` entry is then
//!   the empty-map marker.
//! - Values are canonical CBOR: every map, at every depth, is sorted by its
//!   encoded key bytes. Floats are bit-exact: ciborium only narrows an f64 to
//!   f16/f32 when widening back reproduces the identical bits, including NaN
//!   payloads, `-0.0` and subnormals. There is no text float parse anywhere,
//!   which is the class that breaks attestation signatures under serde_json.
//! - Hash-set fields (`CANONICAL_SET_FIELDS`, test-enforced) are sorted by
//!   canonical element bytes; every other array keeps its order. One logical
//!   state therefore has exactly ONE encoding, so honest exporters agree
//!   byte for byte, which #3126's cross-source checks rely on, and the
//!   importer's round-trip check is plain byte equality.
//!
//! Not carried: [`NODE_LOCAL_STATE_FIELDS`] (reviewed, each with a reason)
//! and the `#[serde(skip)]` runtime handles, which the importer takes from
//! the importing node exactly as the v1 in-place apply did.

use ciborium::value::Value;
use serde::{Serialize, de::DeserializeOwned};

use crate::blockchain::state::{ApplySnapshotError, State};

/// Snapshot entry-format version this build emits and accepts. v1 is refused
/// outright (#3288 decision: no live node uses HTTP ingest).
pub const SNAPSHOT_FORMAT_VERSION: u64 = 2;
pub const NODE_BOND_SNAPSHOT_FORMAT_VERSION: u64 = 3;
pub const NODE_RECEIPT_SNAPSHOT_FORMAT_VERSION: u64 = 4;
pub const NODE_SETTLEMENT_SNAPSHOT_FORMAT_VERSION: u64 = 5;
pub const NODE_FRAUD_SNAPSHOT_FORMAT_VERSION: u64 = 6;

/// Key of the format-version entry.
pub const FORMAT_VERSION_KEY: &str = "meta/format_version";

const FIELD_PREFIX: &str = "state/";

/// Serialized `State` fields that are deliberately NOT carried, with the
/// reason. Every other serialized field is carried; the coverage test in
/// this module fails if a name here is not a serialized field.
pub const NODE_LOCAL_STATE_FIELDS: &[(&str, &str)] = &[(
    "state_root",
    "derived: the importer recomputes it from the carried state, and ingest \
     requires it to equal the signed state_root_hex (#3288 step 2b)",
)];

fn is_node_local(field: &str) -> bool {
    NODE_LOCAL_STATE_FIELDS.iter().any(|(name, _)| *name == field)
}

/// Recursively sort every map by its canonically encoded key bytes.
fn canonicalize(value: Value) -> Result<Value, String> {
    Ok(match value {
        Value::Map(entries) => {
            let mut keyed = entries
                .into_iter()
                .map(|(k, v)| {
                    let k = canonicalize(k)?;
                    let v = canonicalize(v)?;
                    Ok((write_value(&k)?, k, v))
                })
                .collect::<Result<Vec<_>, String>>()?;
            keyed.sort_by(|a, b| a.0.cmp(&b.0));
            Value::Map(keyed.into_iter().map(|(_, k, v)| (k, v)).collect())
        },
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .map(canonicalize)
                .collect::<Result<Vec<_>, String>>()?,
        ),
        Value::Tag(tag, inner) => Value::Tag(tag, Box::new(canonicalize(*inner)?)),
        other => other,
    })
}

fn write_value(value: &Value) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    ciborium::ser::into_writer(value, &mut out).map_err(|e| e.to_string())?;
    Ok(out)
}

fn read_value(bytes: &[u8]) -> Result<Value, String> {
    ciborium::de::from_reader::<Value, _>(bytes).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Type-directed canonical serializer (#3288 review: ONE canonical encoding)
// ---------------------------------------------------------------------------

/// Struct fields whose serde form is an UNORDERED collection (a hash set, or
/// a Vec used as a set). Their arrays are sorted by canonical element bytes at
/// export, so one logical state has exactly one encoding. Keyed by the serde
/// struct name (the Rust name; none of these are renamed).
///
/// Enforced by `every_serialized_hash_set_field_is_classified_3288` (a source
/// scan for hash-set-typed fields of `Serialize` structs) and
/// `set_field_lists_match_what_state_serialization_visits_3288`. A field whose
/// order is semantically meaningful must NOT be listed; listing is an
/// explicit, reviewed choice.
pub const CANONICAL_SET_FIELDS: &[(&str, &str)] = &[
    ("State", "oracle_set"),
    ("State", "privacy_commitments"),
    ("State", "privacy_spent_key_images"),
    ("State", "privacy_spent_nullifiers"),
    ("State", "privacy_disclosure_ids"),
    ("State", "spent_oracle_nonces"),
    ("State", "reward_claim_key_images"),
    ("State", "slashing_evidence_seen"),
    ("State", "gamification_claims_seen"),
    ("GovernanceParameters", "oracle_set"),
    ("ContributorTierPolicy", "trusted_provider_station_ids"),
    ("OracleRegistry", "oracle_set"),
    ("CommitRevealState", "replay_keys"),
];

/// Hash-set fields of `Serialize` structs that `State` does not reach, with
/// the reason. The visit test fails if serializing `State` ever reaches one.
/// Empty today: every serialized hash-set field in these crates is reachable
/// from `State` (the other hash sets, in the mempool, relay queue and
/// validation pipeline, belong to types that do not derive `Serialize`).
pub const SET_FIELDS_NOT_REACHABLE_FROM_STATE: &[(&str, &str, &str)] = &[];

fn is_canonical_set_field(structure: &str, field: &str) -> bool {
    CANONICAL_SET_FIELDS.iter().any(|(s, f)| *s == structure && *f == field)
}

type CanonError = ciborium::value::Error;

/// Records every `(struct, field)` serialized, for the list tests.
type Visited = std::cell::RefCell<std::collections::BTreeSet<(&'static str, &'static str)>>;

#[derive(Clone, Copy)]
struct CanonSer<'a> {
    visited: Option<&'a Visited>,
}

fn sort_set(structure: &str, field: &str, value: Value) -> Result<Value, CanonError> {
    let custom = |why: String| <CanonError as serde::ser::Error>::custom(why);
    match value {
        Value::Array(items) => {
            let mut keyed = items
                .into_iter()
                .map(|item| {
                    let item = canonicalize(item).map_err(&custom)?;
                    Ok((write_value(&item).map_err(&custom)?, item))
                })
                .collect::<Result<Vec<_>, CanonError>>()?;
            keyed.sort_by(|a, b| a.0.cmp(&b.0));
            Ok(Value::Array(keyed.into_iter().map(|(_, v)| v).collect()))
        },
        other => Err(custom(format!(
            "CANONICAL_SET_FIELDS lists {structure}.{field}, but it serialized as {other:?}, \
             not an array"
        ))),
    }
}

struct SeqAcc<'a> {
    ser: CanonSer<'a>,
    items: Vec<Value>,
    variant: Option<&'static str>,
}

struct MapAcc<'a> {
    ser: CanonSer<'a>,
    entries: Vec<(Value, Value)>,
    key: Option<Value>,
}

struct StructAcc<'a> {
    ser: CanonSer<'a>,
    name: &'static str,
    fields: Vec<(Value, Value)>,
    variant: Option<&'static str>,
}

impl<'a> serde::Serializer for CanonSer<'a> {
    type Ok = Value;
    type Error = CanonError;
    type SerializeSeq = SeqAcc<'a>;
    type SerializeTuple = SeqAcc<'a>;
    type SerializeTupleStruct = SeqAcc<'a>;
    type SerializeTupleVariant = SeqAcc<'a>;
    type SerializeMap = MapAcc<'a>;
    type SerializeStruct = StructAcc<'a>;
    type SerializeStructVariant = StructAcc<'a>;

    fn serialize_bool(self, v: bool) -> Result<Value, CanonError> {
        Ok(Value::Bool(v))
    }
    fn serialize_i8(self, v: i8) -> Result<Value, CanonError> {
        Ok(Value::Integer(v.into()))
    }
    fn serialize_i16(self, v: i16) -> Result<Value, CanonError> {
        Ok(Value::Integer(v.into()))
    }
    fn serialize_i32(self, v: i32) -> Result<Value, CanonError> {
        Ok(Value::Integer(v.into()))
    }
    fn serialize_i64(self, v: i64) -> Result<Value, CanonError> {
        Ok(Value::Integer(v.into()))
    }
    fn serialize_i128(self, v: i128) -> Result<Value, CanonError> {
        Value::serialized(&v)
    }
    fn serialize_u8(self, v: u8) -> Result<Value, CanonError> {
        Ok(Value::Integer(v.into()))
    }
    fn serialize_u16(self, v: u16) -> Result<Value, CanonError> {
        Ok(Value::Integer(v.into()))
    }
    fn serialize_u32(self, v: u32) -> Result<Value, CanonError> {
        Ok(Value::Integer(v.into()))
    }
    fn serialize_u64(self, v: u64) -> Result<Value, CanonError> {
        Ok(Value::Integer(v.into()))
    }
    fn serialize_u128(self, v: u128) -> Result<Value, CanonError> {
        Value::serialized(&v)
    }
    // determinism-ok: bit-exact pass-through of a serde float into the snapshot value tree; no arithmetic, and CBOR preserves the bits (see f64_edge_values_round_trip_bit_exactly_3288)
    fn serialize_f32(self, v: f32) -> Result<Value, CanonError> {
        Ok(Value::Float(v.into()))
    }
    // determinism-ok: bit-exact pass-through of a serde float into the snapshot value tree; no arithmetic, and CBOR preserves the bits (see f64_edge_values_round_trip_bit_exactly_3288)
    fn serialize_f64(self, v: f64) -> Result<Value, CanonError> {
        Ok(Value::Float(v))
    }
    fn serialize_char(self, v: char) -> Result<Value, CanonError> {
        Ok(Value::Text(v.to_string()))
    }
    fn serialize_str(self, v: &str) -> Result<Value, CanonError> {
        Ok(Value::Text(v.to_string()))
    }
    fn serialize_bytes(self, v: &[u8]) -> Result<Value, CanonError> {
        Ok(Value::Bytes(v.to_vec()))
    }
    fn serialize_none(self) -> Result<Value, CanonError> {
        Ok(Value::Null)
    }
    fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> Result<Value, CanonError> {
        value.serialize(self)
    }
    fn serialize_unit(self) -> Result<Value, CanonError> {
        Ok(Value::Null)
    }
    fn serialize_unit_struct(self, _name: &'static str) -> Result<Value, CanonError> {
        Ok(Value::Null)
    }
    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
    ) -> Result<Value, CanonError> {
        Ok(Value::Text(variant.to_string()))
    }
    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<Value, CanonError> {
        value.serialize(self)
    }
    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        name: &'static str,
        _index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<Value, CanonError> {
        if name == "@@TAG@@" {
            return Err(<CanonError as serde::ser::Error>::custom(
                "CBOR tags are not part of the snapshot encoding",
            ));
        }
        Ok(Value::Map(vec![(Value::Text(variant.to_string()), value.serialize(self)?)]))
    }
    fn serialize_seq(self, len: Option<usize>) -> Result<SeqAcc<'a>, CanonError> {
        Ok(SeqAcc { ser: self, items: Vec::with_capacity(len.unwrap_or(0)), variant: None })
    }
    fn serialize_tuple(self, len: usize) -> Result<SeqAcc<'a>, CanonError> {
        self.serialize_seq(Some(len))
    }
    fn serialize_tuple_struct(self, _name: &'static str, len: usize) -> Result<SeqAcc<'a>, CanonError> {
        self.serialize_seq(Some(len))
    }
    fn serialize_tuple_variant(
        self,
        name: &'static str,
        _index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<SeqAcc<'a>, CanonError> {
        if name == "@@TAG@@" {
            return Err(<CanonError as serde::ser::Error>::custom(
                "CBOR tags are not part of the snapshot encoding",
            ));
        }
        Ok(SeqAcc { ser: self, items: Vec::with_capacity(len), variant: Some(variant) })
    }
    fn serialize_map(self, len: Option<usize>) -> Result<MapAcc<'a>, CanonError> {
        Ok(MapAcc { ser: self, entries: Vec::with_capacity(len.unwrap_or(0)), key: None })
    }
    fn serialize_struct(self, name: &'static str, len: usize) -> Result<StructAcc<'a>, CanonError> {
        Ok(StructAcc { ser: self, name, fields: Vec::with_capacity(len), variant: None })
    }
    fn serialize_struct_variant(
        self,
        name: &'static str,
        _index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<StructAcc<'a>, CanonError> {
        Ok(StructAcc { ser: self, name, fields: Vec::with_capacity(len), variant: Some(variant) })
    }
    fn is_human_readable(&self) -> bool {
        // Must match ciborium's Value serializer/deserializer.
        false
    }
}

impl serde::ser::SerializeSeq for SeqAcc<'_> {
    type Ok = Value;
    type Error = CanonError;
    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), CanonError> {
        self.items.push(value.serialize(self.ser)?);
        Ok(())
    }
    fn end(self) -> Result<Value, CanonError> {
        let array = Value::Array(self.items);
        Ok(match self.variant {
            Some(variant) => Value::Map(vec![(Value::Text(variant.to_string()), array)]),
            None => array,
        })
    }
}

impl serde::ser::SerializeTuple for SeqAcc<'_> {
    type Ok = Value;
    type Error = CanonError;
    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), CanonError> {
        serde::ser::SerializeSeq::serialize_element(self, value)
    }
    fn end(self) -> Result<Value, CanonError> {
        serde::ser::SerializeSeq::end(self)
    }
}

impl serde::ser::SerializeTupleStruct for SeqAcc<'_> {
    type Ok = Value;
    type Error = CanonError;
    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), CanonError> {
        serde::ser::SerializeSeq::serialize_element(self, value)
    }
    fn end(self) -> Result<Value, CanonError> {
        serde::ser::SerializeSeq::end(self)
    }
}

impl serde::ser::SerializeTupleVariant for SeqAcc<'_> {
    type Ok = Value;
    type Error = CanonError;
    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), CanonError> {
        serde::ser::SerializeSeq::serialize_element(self, value)
    }
    fn end(self) -> Result<Value, CanonError> {
        serde::ser::SerializeSeq::end(self)
    }
}

impl serde::ser::SerializeMap for MapAcc<'_> {
    type Ok = Value;
    type Error = CanonError;
    fn serialize_key<T: ?Sized + Serialize>(&mut self, key: &T) -> Result<(), CanonError> {
        self.key = Some(key.serialize(self.ser)?);
        Ok(())
    }
    fn serialize_value<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), CanonError> {
        let key = self.key.take().ok_or_else(|| {
            <CanonError as serde::ser::Error>::custom("map value without a key")
        })?;
        self.entries.push((key, value.serialize(self.ser)?));
        Ok(())
    }
    fn end(self) -> Result<Value, CanonError> {
        Ok(Value::Map(self.entries))
    }
}

impl StructAcc<'_> {
    fn field<T: ?Sized + Serialize>(&mut self, key: &'static str, value: &T) -> Result<(), CanonError> {
        if let Some(visited) = self.ser.visited {
            visited.borrow_mut().insert((self.name, key));
        }
        let mut value = value.serialize(self.ser)?;
        if self.variant.is_none() && is_canonical_set_field(self.name, key) {
            value = sort_set(self.name, key, value)?;
        }
        self.fields.push((Value::Text(key.to_string()), value));
        Ok(())
    }
    fn finish(self) -> Value {
        let map = Value::Map(self.fields);
        match self.variant {
            Some(variant) => Value::Map(vec![(Value::Text(variant.to_string()), map)]),
            None => map,
        }
    }
}

impl serde::ser::SerializeStruct for StructAcc<'_> {
    type Ok = Value;
    type Error = CanonError;
    fn serialize_field<T: ?Sized + Serialize>(&mut self, key: &'static str, value: &T) -> Result<(), CanonError> {
        self.field(key, value)
    }
    fn end(self) -> Result<Value, CanonError> {
        Ok(self.finish())
    }
}

impl serde::ser::SerializeStructVariant for StructAcc<'_> {
    type Ok = Value;
    type Error = CanonError;
    fn serialize_field<T: ?Sized + Serialize>(&mut self, key: &'static str, value: &T) -> Result<(), CanonError> {
        self.field(key, value)
    }
    fn end(self) -> Result<Value, CanonError> {
        Ok(self.finish())
    }
}

/// Serialize to a canonical `Value`: listed set fields sorted, then every map
/// sorted (`canonicalize`). Arrays that are not listed sets keep their order.
fn to_canonical_value<T: Serialize + ?Sized>(value: &T, visited: Option<&Visited>) -> Result<Value, String> {
    let raw = value.serialize(CanonSer { visited }).map_err(|e| e.to_string())?;
    canonicalize(raw)
}

/// Canonical CBOR bytes for any serializable value.
///
/// Named `*_snapshot_value`, not `*_canonical`, on purpose: this is the
/// snapshot CBOR codec, unrelated to the `crypto::canonical` signing encoding
/// whose decoder `tests/canonical_encoding.rs` keeps out of production.
pub fn encode_snapshot_value<T: Serialize + ?Sized>(value: &T) -> Result<Vec<u8>, String> {
    write_value(&to_canonical_value(value, None)?)
}

/// Decode canonical CBOR bytes produced by [`encode_snapshot_value`].
pub fn decode_snapshot_value<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, String> {
    read_value(bytes)?.deserialized().map_err(|e| e.to_string())
}

fn state_fields(state: &State) -> Result<Vec<(String, Value)>, String> {
    match to_canonical_value(state, None)? {
        Value::Map(fields) => fields
            .into_iter()
            .map(|(k, v)| match k {
                Value::Text(name) => Ok((name, v)),
                other => Err(format!("State field key is not text: {other:?}")),
            })
            .collect(),
        other => Err(format!("State did not serialize as a map: {other:?}")),
    }
}

/// Names of every field serde serializes for `State`, in canonical order.
pub fn serialized_state_field_names() -> Result<Vec<String>, String> {
    Ok(state_fields(&State::default())?.into_iter().map(|(name, _)| name).collect())
}

/// Export `state` as v2 snapshot entries, sorted by key.
pub fn export_state_entries(state: &State) -> Result<Vec<(Vec<u8>, Vec<u8>)>, String> {
    let mut entries = vec![(
        FORMAT_VERSION_KEY.as_bytes().to_vec(),
        (if state.node_fraud.is_some() { NODE_FRAUD_SNAPSHOT_FORMAT_VERSION } else if state.node_settlement.is_some() { NODE_SETTLEMENT_SNAPSHOT_FORMAT_VERSION } else if state.node_receipt_registry.is_some() { NODE_RECEIPT_SNAPSHOT_FORMAT_VERSION } else if state.node_bond_registry.is_some() { NODE_BOND_SNAPSHOT_FORMAT_VERSION } else { SNAPSHOT_FORMAT_VERSION }).to_string().into_bytes(),
    )];
    for (name, value) in state_fields(state)? {
        if is_node_local(&name) || (name == "node_bond_registry" && state.node_bond_registry.is_none()) || (name == "node_receipt_registry" && state.node_receipt_registry.is_none()) || (name == "node_settlement" && state.node_settlement.is_none()) || (name == "node_fraud" && state.node_fraud.is_none()) {
            continue;
        }
        match value {
            Value::Map(items) => {
                entries.push((
                    format!("{FIELD_PREFIX}{name}").into_bytes(),
                    write_value(&Value::Map(Vec::new()))?,
                ));
                for (item_key, item_value) in items {
                    entries.push((
                        format!("{FIELD_PREFIX}{name}/{}", hex::encode(write_value(&item_key)?))
                            .into_bytes(),
                        write_value(&item_value)?,
                    ));
                }
            },
            other => entries.push((
                format!("{FIELD_PREFIX}{name}").into_bytes(),
                write_value(&other)?,
            )),
        }
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(entries)
}

/// Build the `State` a v2 snapshot describes, on top of `local`.
///
/// Fail-closed: a missing or unsupported format version, an unknown key or
/// field, a missing carried field, an item entry for a non-map field, an
/// undecodable value, or a state that does not deserialize are all errors.
/// Node-local fields and the `#[serde(skip)]` runtime handles come from
/// `local`. The returned state's `state_root` is recalculated; comparing it
/// with the signed root is the caller's job (ingest step 2b).
pub fn import_state_entries(
    local: &State,
    entries: &[(Vec<u8>, Vec<u8>)],
) -> Result<State, ApplySnapshotError> {
    use std::collections::BTreeMap;

    let malformed = |key: &str, why: String| ApplySnapshotError::MalformedValue(key.to_string(), why);

    let local_fields = state_fields(local).map_err(|e| malformed("<local state>", e))?;
    let known: BTreeMap<&str, &Value> =
        local_fields.iter().map(|(name, value)| (name.as_str(), value)).collect();

    let mut version: Option<&[u8]> = None;
    // field -> (whole value, per-item entries)
    let mut carried: BTreeMap<String, (Option<Value>, Vec<(Value, Value)>)> = BTreeMap::new();

    for (raw_key, raw_value) in entries {
        let key = std::str::from_utf8(raw_key)
            .map_err(|_| ApplySnapshotError::MalformedKey(hex::encode(raw_key)))?;
        if key == FORMAT_VERSION_KEY {
            if version.replace(raw_value.as_slice()).is_some() {
                return Err(malformed(key, "duplicate format version".into()));
            }
            continue;
        }
        let Some(rest) = key.strip_prefix(FIELD_PREFIX) else {
            return Err(ApplySnapshotError::UnsupportedEntryType(key.to_string()));
        };
        let (field, item) = match rest.split_once('/') {
            Some((field, item_hex)) => (field, Some(item_hex)),
            None => (rest, None),
        };
        if !known.contains_key(field) || is_node_local(field) {
            return Err(ApplySnapshotError::UnsupportedEntryType(key.to_string()));
        }
        let value = read_value(raw_value).map_err(|e| malformed(key, e))?;
        let slot = carried.entry(field.to_string()).or_default();
        match item {
            None => {
                if slot.0.replace(value).is_some() {
                    return Err(malformed(key, "duplicate field entry".into()));
                }
            },
            Some(item_hex) => {
                let item_key = hex::decode(item_hex)
                    .map_err(|_| ApplySnapshotError::MalformedKey(key.to_string()))
                    .and_then(|bytes| read_value(&bytes).map_err(|e| malformed(key, e)))?;
                slot.1.push((item_key, value));
            },
        }
    }

    match version {
        Some(v) if v == SNAPSHOT_FORMAT_VERSION.to_string().as_bytes() || v == NODE_BOND_SNAPSHOT_FORMAT_VERSION.to_string().as_bytes() || v == NODE_RECEIPT_SNAPSHOT_FORMAT_VERSION.to_string().as_bytes() || v == NODE_SETTLEMENT_SNAPSHOT_FORMAT_VERSION.to_string().as_bytes() || v == NODE_FRAUD_SNAPSHOT_FORMAT_VERSION.to_string().as_bytes() => {},
        Some(v) => {
            return Err(ApplySnapshotError::UnsupportedEntryType(format!(
                "{FORMAT_VERSION_KEY}={} (this node accepts {SNAPSHOT_FORMAT_VERSION})",
                String::from_utf8_lossy(v)
            )));
        },
        None => {
            return Err(ApplySnapshotError::UnsupportedEntryType(format!(
                "missing {FORMAT_VERSION_KEY}: a v1 snapshot is not accepted (#3288)"
            )));
        },
    }

    let mut fields: Vec<(Value, Value)> = Vec::with_capacity(local_fields.len());
    for (name, local_value) in &local_fields {
        let value = if name == "node_bond_registry" && version == Some(b"2".as_slice()) {
            if carried.contains_key(name) { return Err(malformed(name, "Node marker in legacy snapshot".into())); }
            Value::Null
        } else if name == "node_fraud" && version != Some(b"6".as_slice()) {
            if carried.contains_key(name) { return Err(malformed(name, "sanctions marker in legacy snapshot".into())); }
            Value::Null
        } else if name == "node_settlement" && version != Some(b"5".as_slice()) && version != Some(b"6".as_slice()) {
            if carried.contains_key(name) { return Err(malformed(name, "settlement marker in legacy snapshot".into())); }
            Value::Null
        } else if name == "node_receipt_registry" && version != Some(b"4".as_slice()) && version != Some(b"5".as_slice()) && version != Some(b"6".as_slice()) {
            if carried.contains_key(name) { return Err(malformed(name, "receipt marker in legacy snapshot".into())); }
            Value::Null
        } else if is_node_local(name) {
            (*local_value).clone()
        } else {
            let key = format!("{FIELD_PREFIX}{name}");
            let (whole, items) = carried.remove(name).unwrap_or_default();
            let whole = whole.ok_or_else(|| {
                malformed(&key, "carried State field missing from the snapshot".into())
            })?;
            if items.is_empty() {
                whole
            } else {
                match whole {
                    Value::Map(marker) if marker.is_empty() => Value::Map(items),
                    _ => return Err(malformed(&key, "item entries for a non-map field".into())),
                }
            }
        };
        fields.push((Value::Text(name.clone()), value));
    }

    let mut installed: State = canonicalize(Value::Map(fields))
        .map_err(|e| malformed("<state>", e))?
        .deserialized()
        .map_err(|e| malformed("<state>", e.to_string()))?;
    installed.adopt_runtime_fields_from(local);
    installed.restore_node_emission_runtime().map_err(|e| malformed("emission_ledger", e.to_string()))?;
    if (version == Some(b"3".as_slice()) || version == Some(b"4".as_slice()) || version == Some(b"5".as_slice()) || version == Some(b"6".as_slice())) && installed.node_bond_registry.is_none() {
        return Err(malformed("node_bond_registry", "missing activated marker".into()));
    }
    if (version == Some(b"4".as_slice()) || version == Some(b"5".as_slice()) || version == Some(b"6".as_slice())) && installed.node_receipt_registry.is_none() { return Err(malformed("node_receipt_registry", "missing receipt marker".into())); }
    if (version == Some(b"5".as_slice()) || version == Some(b"6".as_slice())) && installed.node_settlement.is_none() { return Err(malformed("node_settlement", "missing settlement marker".into())); }
    if version == Some(b"6".as_slice()) && installed.node_fraud.is_none() { return Err(malformed("node_fraud", "missing sanctions marker".into())); }
    installed.validate_node_bond_anchor(installed.emission_ledger.height()).map_err(|e| malformed("node_bond_registry", e.to_string()))?;
    installed.validate_node_finalized(installed.emission_ledger.height()).map_err(|e| malformed("node_settlement", e.to_string()))?;
    installed.recalculate_state_root();
    Ok(installed)
}

/// Two canonical values are equal iff they encode to the same bytes (#3288
/// review: one logical state, one encoding, so there is no order-insensitive
/// fallback anywhere).
pub fn equivalent(a: &Value, b: &Value) -> bool {
    matches!((write_value(a), write_value(b)), (Ok(x), Ok(y)) if x == y)
}

/// Whole-`State` equivalence over everything the snapshot carries (all
/// serialized fields except [`NODE_LOCAL_STATE_FIELDS`]). Returns the first
/// differing field name.
pub fn first_carried_difference(a: &State, b: &State) -> Result<Option<String>, String> {
    let (fa, fb) = (state_fields(a)?, state_fields(b)?);
    for ((name, va), (name_b, vb)) in fa.iter().zip(&fb) {
        if name != name_b {
            return Ok(Some(format!("field order: {name} vs {name_b}")));
        }
        if !is_node_local(name) && !equivalent(va, vb) {
            return Ok(Some(name.clone()));
        }
    }
    Ok((fa.len() != fb.len()).then(|| "field count".to_string()))
}

/// Compare two v2 entry lists byte for byte (after sorting by key); returns
/// the first differing key. This is ingest's round-trip check. The encoding is
/// canonical, so byte equality is the right test.
pub fn first_entry_difference(
    a: &[(Vec<u8>, Vec<u8>)],
    b: &[(Vec<u8>, Vec<u8>)],
) -> Option<String> {
    let label = |key: &[u8]| String::from_utf8_lossy(key).into_owned();
    let (mut a, mut b) = (a.to_vec(), b.to_vec());
    a.sort_by(|x, y| x.0.cmp(&y.0));
    b.sort_by(|x, y| x.0.cmp(&y.0));
    for (i, ((ka, va), (kb, vb))) in a.iter().zip(&b).enumerate() {
        if ka != kb {
            return Some(format!("key #{i}: {} vs {}", label(ka), label(kb)));
        }
        if va != vb {
            return Some(label(ka));
        }
    }
    (a.len() != b.len()).then(|| format!("entry count {} vs {}", a.len(), b.len()))
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::blockchain::state::{
        Account, AuditEventKind, AuditEventRecord, CalibrationAttestationRecord, ComputationalNFT,
        ContributionRewardRecord, MiningPoolRecord, QualityPolicyEventArchiveBucket,
        QualityPolicyEventRecord, QualityPolicyEventType, StateTransition,
    };
    use crate::common::types::{Address, Balance, NFTId, NFTMetadata};
    use crate::contributions::commit_reveal::{CommitRecord, RevealRecord};
    use crate::oracle::attestation::{
        AttestationRecord, AttestationSet, AttesterSettlementDebt, AttesterSettlementRecord,
        ProducerSettlementRecord,
    };
    use crate::station_registry::{
        ContributionProvenanceClass, DataQualityTier, StationLocation, WeatherStation,
    };
    use crate::weather::validation::{DataQualityBand, WeatherObservation};

    /// f64 values whose bits must survive the codec exactly.
    const F64_EDGE_BITS: &[u64] = &[
        0x7ff8_0000_dead_beef, // quiet NaN with a payload
        0x7ff0_0000_0000_0001, // signalling NaN
        0xfff8_0000_0000_0000, // negative quiet NaN
        0x8000_0000_0000_0000, // -0.0
        0x0000_0000_0000_0000, // +0.0
        0x0000_0000_0000_0001, // smallest subnormal
        0x000f_ffff_ffff_ffff, // largest subnormal
        0x7ff0_0000_0000_0000, // +inf
        0xfff0_0000_0000_0000, // -inf
        0x3fd5_5555_5555_5555, // 1/3: needs all 64 bits
        0x3fb9_9999_9999_999a, // 0.1
        0x7fef_ffff_ffff_ffff, // f64::MAX
    ];

    /// Carried fields the fixture below leaves at their `State::default()`
    /// value, so the full round trip does not exercise their populated form.
    /// Kept honest by `fixture_populates_every_field_not_listed_default_3288`
    /// in BOTH directions.
    const FIXTURE_DEFAULT_FIELDS: &[&str] = &[];

    fn a(byte: u8) -> Address {
        Address::new([byte; 20])
    }

    fn rich_state() -> State {
        let mut s = State::default();
        // `commit_reveal_state` is private to `state`; populate it the way a
        // block does. FIRST: it rebuilds the pending_commits / pending_reveals
        // views, so the direct inserts below must come after it.
        let mut commit = crate::blockchain::transaction::CommitPayload {
            commit_id: "c2".repeat(32),
            station_id: "PWS-3288-01".into(),
            timestamp: 1_700_000_000,
            contribution_type: "contribution".into(),
            auth_pubkey: None,
            auth_signature: None,
        };
        commit.sign_authorization(&crate::crypto::signature::PrivateKey::from_bytes(&[0x32; 32]).unwrap());
        s.apply_transitions_batch(std::iter::once(StateTransition::RegisterCommit {
            payload: commit,
            block_height: 12,
        }))
        .expect("commit registers");


        let mut account = Account::new(Balance(1_000));
        account.nonce = 3;
        account.code = Some(vec![0x60, 0x00, 0x32, 0x88]);
        account.is_contract = true;
        account.storage.insert(vec![1, 2], vec![3, 4]);
        account.stake_required = Balance(7);
        account.stake_locked = Balance(5);
        account.spendable_balance = Balance(900);
        account.earned_balance_total = Balance(40);
        s.accounts.insert(a(7), account);

        let nft_id = NFTId("nft-3288".into());
        s.nfts.insert(
            nft_id.clone(),
            ComputationalNFT {
                id: nft_id,
                owner: a(8),
                creator: a(9),
                metadata: NFTMetadata {
                    name: "n".into(),
                    description: Some("d".into()),
                    data_type: "t".into(),
                    region: Some("r".into()),
                    accuracy: Some(0.5),
                    creation_time: serde_json::from_value(serde_json::json!(1_700_000_000u64)).unwrap(),
                    creator: a(9),
                    metadata_uri: Some("u".into()),
                    properties: serde_json::json!({"k": ["v", 1, 2.5, null, {"deep": true}]}),
                },
                price: Some(Balance(5)),
                verified: true,
                verification_count: 2,
                avg_confidence: Some(f64::from_bits(F64_EDGE_BITS[0])),
                created_at: 1_700_000_000,
                updated_at: 1_700_000_100,
            },
        );

        s.pending_commits.insert(
            "c1".into(),
            CommitRecord {
                commit_id: "c1".into(),
                station_id: "PWS-1".into(),
                contribution_type: "contribution".into(),
                provenance_class: ContributionProvenanceClass::Real,
                committed_at: 10,
                commit_expires_at: 20,
                reveal_expires_at: 30,
            },
        );
        s.pending_reveals.insert(
            "c1".into(),
            RevealRecord {
                commit_id: "c1".into(),
                station_id: "PWS-1".into(),
                provenance_class: ContributionProvenanceClass::Real,
                revealed_at: 11,
                reveal_id: "r1".into(),
                canonical_calibration_attestation_cid: Some("bafy3288".into()),
                quality_score: None,
            },
        );
        s.contribution_reward_records.insert(
            "c1".into(),
            ContributionRewardRecord {
                address: a(7),
                amount: Balance(12),
                reason: "contribution:c1".into(),
                station_id: "PWS-1".into(),
                provenance_class: ContributionProvenanceClass::Real,
            },
        );
        s.finalized_contribution_rewards.insert("c1".into());

        s.apply_transitions_batch(std::iter::once(StateTransition::ApplyOracleRegistration {
            oracle_set: vec!["a1".repeat(32)],
            oracle_quorum: 1,
            identity: "a1".repeat(32),
            public_key: "a1".repeat(32),
            epoch: 1,
            activated_at: 1_700_000_000,
        }))
        .expect("oracle registration applies");

        let mut set = AttestationSet::default();
        set.by_oracle.insert(
            "o1".into(),
            AttestationRecord { oracle_id: "o1".into(), approved: true, confidence: 0.75, timestamp: 99 },
        );
        s.attestations.insert("c1".into(), set);

        for (i, id) in ["x1", "x2", "x3"].iter().enumerate() {
            s.privacy_commitments.insert((*id).into());
            s.privacy_commitment_amounts.insert((*id).into(), 100 + i as u128);
            s.privacy_commitments_height.insert((*id).into(), 5 + i as u64);
            s.privacy_spent_key_images.insert((*id).into());
            s.privacy_spent_key_images_height.insert((*id).into(), 6);
            s.privacy_spent_nullifiers.insert((*id).into());
            s.privacy_spent_nullifiers_height.insert((*id).into(), 7);
            s.privacy_disclosure_ids.insert((*id).into());
            s.privacy_disclosure_ids_height.insert((*id).into(), 8);
            s.spent_oracle_nonces.insert((*id).into());
            s.spent_oracle_nonces_height.insert((*id).into(), 9);
            s.reward_claim_key_images.insert((*id).into());
            s.reward_claim_key_images_height.insert((*id).into(), 10);
            s.gamification_claims_seen.insert((*id).into());
            s.zk_verified_proofs.insert((*id).into(), 11);
        }
        s.last_state_pruning_height = 7;
        s.feeless_worker_epoch_counts.insert("w1".into(), 3);

        s.governance.params.commit_window_blocks += 1;
        s.governance.params.weather_strict_v1_observation_keys = true;
        s.governance.treasury_balance = 3288;
        s.governance.delegations.insert("d1".into(), "d2".into());

        s.producer_set_history
            .push(crate::consensus::producer_set::ProducerSetChange::new(1, 42, Vec::new()));
        s.low_quality_streaks.insert(a(7), 2);

        let policy = s.governance.params.data_quality_policy.clone();
        s.quality_policy_events.push(QualityPolicyEventRecord {
            event_id: 1,
            event_type: QualityPolicyEventType::ChallengeTriggered,
            block_height: 4,
            commit_id: "c1".into(),
            participant: a(7),
            quality_band: DataQualityBand::High,
            quality_score: None,
            scoring_multiplier: f64::from_bits(F64_EDGE_BITS[3]),
            reward_multiplier: f64::from_bits(F64_EDGE_BITS[5]),
            warning_only: true,
            trigger_challenge: true,
            counts_toward_low_quality_streak: true,
            low_quality_streak: Some(1),
            low_quality_slash_threshold: Some(3),
            slashed_amount: Some(Balance(2)),
            policy_snapshot: policy,
        });
        s.audit_events.push(AuditEventRecord {
            event_id: 1,
            block_height: 4,
            timestamp: 1_700_000_000,
            kind: AuditEventKind::GovernanceProposalCreated,
            subject_id: Some("p1".into()),
            actor_id: None,
            details: Some(serde_json::json!({"x": 1})),
        });
        s.quality_policy_event_archive.push(QualityPolicyEventArchiveBucket {
            block_height: 3,
            first_event_id: 1,
            last_event_id: 2,
            total_events: 2,
            challenge_events: 1,
            slashing_events: 0,
            band_counters: Default::default(),
            challenge_band_counters: Default::default(),
            slashing_band_counters: Default::default(),
        });
        s.archived_quality_policy_event_count = 2;
        s.archived_quality_policy_event_through_event_id = Some(2);

        s.mining_pools.insert(
            "pool1".into(),
            MiningPoolRecord {
                id: "pool1".into(),
                name: "p".into(),
                operator: "0x07".into(),
                fee_percentage: 3,
                created_at_block: 2,
                members: vec!["m1".into(), "m2".into()],
            },
        );
        s.calibration_attestations.insert(
            "cal1".into(),
            CalibrationAttestationRecord {
                station_id: "PWS-1".into(),
                calibrator_id: "cal".into(),
                calibrator_key_epoch: Some(1),
                issued_at: 1,
                expires_at: 2,
                nonce: Some("n".into()),
            },
        );
        s.station_registry
            .register(WeatherStation {
                station_id: "PWS-3288-01".into(),
                owner_address: "0".repeat(40),
                location: StationLocation { latitude: 46.79, longitude: -92.1, elevation_meters: None },
                hardware_model: "test".into(),
                calibration_date: None,
                is_active: true,
                reputation_score: 0,
                data_quality_tier: DataQualityTier::Unverified,
                provenance_class: ContributionProvenanceClass::Real,
                ring_public_key: None,
            })
            .expect("station registers");
        let cell = wrfcoin_geo_grid::GeoCell { lat_idx: 4679, lon_idx: -9210, resolution_km: 1 };
        s.station_counts.insert(cell, 2);
        s.recent_weather_observations.insert(
            "PWS-1".into(),
            (
                WeatherObservation {
                    temperature_c: Some(f64::from_bits(F64_EDGE_BITS[9])),
                    pressure_hpa: Some(1013.25),
                    ..Default::default()
                },
                100,
            ),
        );
        for (i, bits) in F64_EDGE_BITS.iter().enumerate() {
            s.spatial_scores.insert(format!("edge-{i:02}"), f64::from_bits(*bits));
        }
        s.oracle_local_accuracy.insert(
            ("o1".into(), cell),
            crate::consensus::hyper_local::OracleLocalAccuracy {
                oracle_id: "o1".into(),
                cell,
                recent_scores: [1u32, 2, 3].into_iter().collect(),
                receipt_count: 3,
            },
        );

        s.supply_genesis = 1_000_000;
        s.supply_authorized_minting = 500;
        s.supply_authorized_burning = 20;
        s.supply_baseline_initialized = true;
        s.shielded_pool = 303;
        s.supply_violation = Some("fixture".into());
        s.slashing_reason_counts
            .insert(crate::security::slashing::SlashingReason::InvalidReveal, 2);
        s.slashing_evidence_seen.insert([3; 32]);
        s.slashing_evidence_seen_height.insert([3; 32], 4);
        s.emission_window_start = 3;
        s.emission_window_spent.insert(a(7), 99);
        s.attester_settlement_due.insert(
            "d1".into(),
            AttesterSettlementDebt { crossing_height: 5, eligible: vec!["o1".into()] },
        );
        s.attester_settlements.insert(
            "d0".into(),
            AttesterSettlementRecord { decision_id: "d0".into(), finalization_height: 4, allocations: Vec::new() },
        );
        s.genesis_emission_config = Some(crate::crypto::chain_binding::GenesisEmissionConfig {
            chain_id: "777".into(),
            faucet_address: Some(a(37)),
         ..Default::default() });
        s.last_producer_settlement = Some(ProducerSettlementRecord {
            height: 9,
            payout_address: "0xC1C1C1C1C1C1C1C1C1C1C1C1C1C1C1C1C1C1C1C1".into(),
            amount: 42,
        });
        s.account_recovery.insert(a(7), Default::default());
        s.session_keys.insert(a(7), Default::default());
        s.authorization_declarations.insert(a(7), Default::default());
        s.paymasters.insert(
            a(7),
            crate::accounts::paymaster::PaymasterAccount {
                deposit: 5,
                unstake_delay_blocks: 10,
                unstake_requested_at: Some(3),
                epoch_started_at: 1,
                sponsorships_this_epoch: 2,
                successes_this_epoch: 1,
                failures_this_epoch: 1,
                throttled_until: None,
                per_sender: Default::default(),
            },
        );
        s.emission_ledger =
            crate::consensus::proof_of_publishing::EmissionPartitionLedger::open(5, Some("777"));
        // #3277 S2. Every inner field is non-default, `version_steps` included
        // (always empty on chain until S11, but the codec must carry it).
        {
            use crate::governance::schedule_extension::{
                AppliedExtension, ScheduleEntry, VersionStep,
            };
            s.feature_schedule_extensions.push(AppliedExtension {
                sequence: 1,
                carried_at_height: 12,
                entries: vec![ScheduleEntry::new("schedule_extension_fixture_3288", 40)],
                version_steps: vec![VersionStep {
                    activation_height: 40,
                    version: 2,
                }],
                min_protocol_version: 2,
                release_commit: "release-fixture-3288".into(),
                signers: vec!["ab".repeat(32)],
            });
        }
        // #3270 S9: the accepting block's header timestamp per station.
        s.station_weather_accepted_block_unix
            .insert("station-3288".to_string(), 1_790_000_000);
        // #3345 / #3347. Every inner field non-default.
        {
            use crate::blockchain::station_owner::{
                PendingStationOwnerRecovery, StationFreeze, StationOwnerLedgerEntry,
            };
            s.station_owner_ledger.insert(
                "station-fixture-3345".into(),
                StationOwnerLedgerEntry {
                    owner_nonce: 3,
                    frozen: Some(StationFreeze { since_height: 11 }),
                    owner_deactivated: true,
                    pending_recovery: Some(PendingStationOwnerRecovery {
                        from_owner_address: "wrf-from-fixture".into(),
                        to_owner_address: "wrf-to-fixture".into(),
                        recovery_nonce: 3,
                        filed_at_height: 12,
                    }),
                },
            );
        }

        s.feature_schedule_extensions[0].entries.push(crate::governance::schedule_extension::ScheduleEntry::new("node_share_settlement", 5));
        s.node_bond_registry = Some(crate::consensus::node_bond_registry::NodeBondRegistry::begin(5, [8; 32], 5, [9; 32]).unwrap());
        s.node_receipt_registry = Some(crate::consensus::node_receipt_registry::NodeReceiptRegistry { block_height:5, ..Default::default() });
        s.node_settlement = Some(crate::consensus::node_settlement::NodeSettlement { last_finalized_height:Some(5), ..Default::default() });
        s.node_fraud = Some(crate::consensus::node_fraud::NodeFraudState { block_height:5, ..Default::default() });
        s
    }

    fn carried_field_values(state: &State) -> Vec<(String, Value)> {
        state_fields(state)
            .expect("state serializes")
            .into_iter()
            .filter(|(name, _)| !is_node_local(name))
            .collect()
    }

    /// Every serialized hash-set field of a `Serialize` struct in the crates
    /// `State` is built from must be classified: sorted at export
    /// (`CANONICAL_SET_FIELDS`) or proven unreachable from `State`
    /// (`SET_FIELDS_NOT_REACHABLE_FROM_STATE`). A new HashSet field fails
    /// here until someone decides which.
    #[test]
    fn every_serialized_hash_set_field_is_classified_3288() {
        use std::collections::BTreeSet;
        fn rs_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            for entry in std::fs::read_dir(dir).expect("read source dir") {
                let path = entry.expect("dir entry").path();
                if path.is_dir() {
                    rs_files(&path, out);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    out.push(path);
                }
            }
        }
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let mut files = Vec::new();
        for crate_src in ["shared-protocol/src", "geo-grid/src", "primitives/src"] {
            let dir = root.join(crate_src);
            if dir.is_dir() {
                rs_files(&dir, &mut files);
            }
        }
        files.sort();
        let is_set_type = |ty: &str| {
            ["HashSet<", "FxHashSet<", "AHashSet<", "ImHashSet<", "im::HashSet<"]
                .iter()
                .any(|needle| ty.contains(needle))
        };
        let mut found = BTreeSet::new();
        let mut nested = Vec::new();
        for file in &files {
            let text = std::fs::read_to_string(file).expect("read source");
            let lines: Vec<&str> = text.lines().collect();
            for (i, line) in lines.iter().enumerate() {
                let trimmed = line.trim_start();
                let Some(rest) = trimmed
                    .strip_prefix("pub struct ")
                    .or_else(|| trimmed.strip_prefix("pub(crate) struct "))
                    .or_else(|| trimmed.strip_prefix("struct "))
                else {
                    continue;
                };
                if !trimmed.contains('{') {
                    continue;
                }
                let name: String =
                    rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
                // The attribute block directly above the struct.
                let mut attrs = String::new();
                for above in lines[..i].iter().rev() {
                    let t = above.trim();
                    if t.starts_with("#[") || t.starts_with("///") || t.starts_with("//") || t.ends_with(')') || t.ends_with(",") {
                        attrs.push_str(t);
                    } else {
                        break;
                    }
                }
                if !attrs.contains("Serialize") {
                    continue;
                }
                let indent = line.len() - trimmed.len();
                let mut skip_next = false;
                for field_line in &lines[i + 1..] {
                    let ft = field_line.trim_start();
                    if ft.starts_with('}') && field_line.len() - ft.len() == indent {
                        break;
                    }
                    if ft.starts_with("#[serde(skip") {
                        skip_next = true;
                        continue;
                    }
                    if ft.starts_with("#[") || ft.starts_with("//") || ft.is_empty() {
                        continue;
                    }
                    let decl = ft
                        .trim_start_matches("pub(crate) ")
                        .trim_start_matches("pub(super) ")
                        .trim_start_matches("pub ");
                    let Some((field, ty)) = decl.split_once(':') else {
                        skip_next = false;
                        continue;
                    };
                    let field = field.trim();
                    if skip_next || !field.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') {
                        skip_next = false;
                        continue;
                    }
                    let ty = ty.trim();
                    if is_set_type(ty) {
                        let outer_is_set = ["HashSet<", "FxHashSet<", "AHashSet<", "ImHashSet<", "im::HashSet<", "std::collections::HashSet<", "rustc_hash::FxHashSet<"]
                            .iter()
                            .any(|p| ty.starts_with(p));
                        if outer_is_set {
                            found.insert((name.clone(), field.to_string()));
                        } else {
                            nested.push(format!("{name}.{field}: {ty} ({})", file.display()));
                        }
                    }
                }
            }
        }
        assert!(
            nested.is_empty(),
            "hash sets nested inside another type need a sort shape this codec does not \
             implement yet (#3288): {nested:?}"
        );
        let listed: BTreeSet<(String, String)> = CANONICAL_SET_FIELDS
            .iter()
            .map(|(s, f)| (s.to_string(), f.to_string()))
            .chain(SET_FIELDS_NOT_REACHABLE_FROM_STATE.iter().map(|(s, f, _)| (s.to_string(), f.to_string())))
            .collect();
        let unlisted: Vec<_> = found.difference(&listed).collect();
        let stale: Vec<_> = listed.difference(&found).collect();
        assert!(
            unlisted.is_empty(),
            "serialized hash-set field(s) {unlisted:?} are not classified. Add each to \
             CANONICAL_SET_FIELDS (sorted at export) or, if State cannot reach it, to \
             SET_FIELDS_NOT_REACHABLE_FROM_STATE (#3288)"
        );
        assert!(stale.is_empty(), "classified field(s) {stale:?} no longer exist as hash-set fields");
    }

    /// The classification matches reality: serializing a fully populated
    /// State visits every CANONICAL_SET_FIELDS entry and none of the
    /// "not reachable" ones.
    #[test]
    fn set_field_lists_match_what_state_serialization_visits_3288() {
        let visited = Visited::default();
        to_canonical_value(&rich_state(), Some(&visited)).expect("serializes");
        let visited = visited.into_inner();
        for (s, f) in CANONICAL_SET_FIELDS {
            assert!(visited.contains(&(s, f)), "{s}.{f} is listed canonical but State never serializes it");
        }
        for (s, f, why) in SET_FIELDS_NOT_REACHABLE_FROM_STATE {
            assert!(!visited.contains(&(s, f)), "{s}.{f} is listed unreachable ({why}) but State serializes it");
        }
    }

    /// One logical state, one encoding: equal hash sets with different bucket
    /// histories (top-level, and nested inside a struct) export byte-identical.
    #[test]
    fn hash_sets_export_canonically_regardless_of_history_3288() {
        let build = |churn: bool| {
            let mut s = State::default();
            let fill = |set: &mut rustc_hash::FxHashSet<String>| {
                if churn {
                    for i in 0..3000 {
                        set.insert(format!("tmp-{i}"));
                    }
                    set.retain(|k| !k.starts_with("tmp-"));
                }
                let ids: Vec<usize> = if churn { (0..40).rev().collect() } else { (0..40).collect() };
                for i in ids {
                    set.insert(format!("item-{i:02}"));
                }
            };
            fill(&mut s.spent_oracle_nonces);
            fill(&mut s.privacy_commitments);
            fill(&mut s.oracle_set);
            fill(&mut s.governance.params.oracle_set);
            fill(&mut s.governance.params.contributor_tier_policy.trusted_provider_station_ids);
            s
        };
        let (fresh, churned) = (
            export_state_entries(&build(false)).unwrap(),
            export_state_entries(&build(true)).unwrap(),
        );
        let first_diff = fresh
            .iter()
            .zip(&churned)
            .find(|(x, y)| x != y)
            .map(|(x, _)| String::from_utf8_lossy(&x.0).into_owned());
        assert_eq!(first_diff, None, "hash sets must export as canonical bytes");
    }

    /// Moving a field out of the snapshot is a reviewed change: it must be
    /// visible in a diff here, not just in the list.
    #[test]
    fn node_local_list_is_pinned_3288() {
        let names: Vec<&str> = NODE_LOCAL_STATE_FIELDS.iter().map(|(n, _)| *n).collect();
        assert_eq!(
            names,
            ["state_root"],
            "NODE_LOCAL_STATE_FIELDS changed: a field that is not carried keeps the \
             importing node's value, which is the #3288 divergence class. Justify \
             it in review, then update this pin."
        );
    }

    /// Maps are canonical: logically equal states whose maps were built in
    /// different insertion orders, including a std `HashMap` with its per-
    /// instance random seed, export byte-identical entries.
    #[test]
    fn maps_export_canonically_regardless_of_insertion_order_3288() {
        use crate::security::slashing::SlashingReason;
        let record = |i: usize| crate::oracle::attestation::AttestationRecord {
            oracle_id: format!("oracle-{i:02}"),
            approved: i % 2 == 0,
            confidence: 0.5,
            timestamp: i as i64,
        };
        let build = |reverse: bool| {
            let mut s = State::default();
            let reasons = [
                SlashingReason::InvalidReveal,
                SlashingReason::OracleAttestationFraud,
                SlashingReason::RepeatedLowQualityData,
            ];
            let mut order: Vec<usize> = (0..300).collect();
            if reverse {
                order.reverse();
            }
            for (n, reason) in reasons.iter().enumerate() {
                let idx = if reverse { reasons.len() - 1 - n } else { n };
                s.slashing_reason_counts.insert(reasons[idx].clone(), idx as u64 + 1);
                let _ = reason;
            }
            // A map NESTED inside a value (attestations -> by_oracle): its
            // serialized order is its bucket order, which depends on history.
            // One side grows the map far past its final size first.
            let mut set = crate::oracle::attestation::AttestationSet::default();
            if reverse {
                for i in 0..4000 {
                    set.by_oracle.insert(format!("tmp{i}"), record(i));
                }
                set.by_oracle.retain(|k, _| !k.starts_with("tmp"));
            }
            let ids: Vec<usize> = if reverse { (0..12).rev().collect() } else { (0..12).collect() };
            for i in ids {
                set.by_oracle.insert(format!("oracle-{i:02}"), record(i));
            }
            s.attestations.insert("c-nested".into(), set);
            for i in order {
                s.feeless_worker_epoch_counts.insert(format!("w{i:03}"), i as u32);
                let mut bytes = [0u8; 20];
                bytes[..2].copy_from_slice(&(i as u16).to_be_bytes());
                s.low_quality_streaks.insert(Address::new(bytes), i as u32);
            }
            s
        };
        let (forward, reverse) =
            (export_state_entries(&build(false)).unwrap(), export_state_entries(&build(true)).unwrap());
        let first_diff = forward
            .iter()
            .zip(&reverse)
            .find(|(x, y)| x != y)
            .map(|(x, _)| String::from_utf8_lossy(&x.0).into_owned());
        assert_eq!(first_diff, None, "map entries must be canonical bytes");
        assert_eq!(forward.len(), reverse.len());
    }

    #[test]
    fn node_local_fields_are_real_serialized_fields_3288() {
        let names = serialized_state_field_names().expect("field names");
        for (name, why) in NODE_LOCAL_STATE_FIELDS {
            assert!(names.iter().any(|n| n == name), "NODE_LOCAL `{name}` ({why}) is not a serialized State field");
        }
        // Every other serialized field is emitted: coverage is serde-driven,
        // so a NEW State field is carried without anyone listing it.
        // Activated layout must carry the new field; the legacy OFF export
        // is separately pinned to omit the absent Node marker byte-for-byte.
        let entries = export_state_entries(&rich_state()).expect("export");
        for name in names.iter().filter(|n| !is_node_local(n)) {
            let key = format!("{FIELD_PREFIX}{name}");
            assert!(entries.iter().any(|(k, _)| k == key.as_bytes()), "field `{name}` is not exported");
        }
    }

    /// `adopt_runtime_fields_from` must copy exactly the `#[serde(skip)]`
    /// fields of `State`, the only State content v2 does not carry besides
    /// NODE_LOCAL_STATE_FIELDS.
    #[test]
    fn runtime_fields_are_exactly_the_serde_skipped_ones_3288() {
        let src = include_str!("state.rs");
        let start = src.find("pub struct State {").expect("State struct");
        let body = &src[start..start + src[start..].find("\n}\n").expect("end of State")];
        let mut skipped = Vec::new();
        let lines: Vec<&str> = body.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            if line.trim_start().starts_with("#[serde(skip") {
                let field = lines[i + 1..]
                    .iter()
                    .map(|l| l.trim_start())
                    .find(|l| !l.starts_with("#[") && !l.starts_with("///") && !l.is_empty())
                    .expect("field after attribute");
                let name = field.trim_start_matches("pub ").split(':').next().unwrap().trim();
                skipped.push(name.to_string());
            }
        }
        let adopt_start = src.find("fn adopt_runtime_fields_from(").expect("adopt fn");
        let adopt = &src[adopt_start..adopt_start + src[adopt_start..].find("\n    }\n").unwrap()];
        let mut adopted: Vec<String> = adopt
            .lines()
            .filter_map(|l| l.trim().strip_prefix("self.").and_then(|r| r.split(" =").next()))
            .map(str::to_string)
            .collect();
        skipped.sort();
        adopted.sort();
        assert!(!skipped.is_empty());
        assert_eq!(skipped, adopted, "runtime handles adopted from the importing node must be exactly the #[serde(skip)] fields");
    }

    #[test]
    fn fixture_populates_every_field_not_listed_default_3288() {
        let default_fields = carried_field_values(&State::default());
        let rich_fields = carried_field_values(&rich_state());
        let mut left_default = Vec::new();
        for ((name, d), (_, r)) in default_fields.iter().zip(&rich_fields) {
            if equivalent(d, r) {
                left_default.push(name.clone());
            }
        }
        let listed: Vec<String> = FIXTURE_DEFAULT_FIELDS.iter().map(|s| s.to_string()).collect();
        assert_eq!(
            left_default, listed,
            "FIXTURE_DEFAULT_FIELDS must list exactly the carried fields the rich fixture leaves at default"
        );
    }

    #[test]
    fn whole_state_round_trips_with_full_equivalence_3288() {
        let mut source = rich_state();
        source.recalculate_state_root();
        let entries = export_state_entries(&source).expect("export");

        let mut synced = State::default();
        synced.apply_snapshot_entries(&entries).expect("import");
        assert_eq!(first_carried_difference(&source, &synced).unwrap(), None);
        assert_eq!(synced.state_root, source.state_root, "state root must survive");

        // And re-export is equivalent, which is ingest's step-2 check.
        let again = export_state_entries(&synced).expect("re-export");
        assert_eq!(first_entry_difference(&entries, &again), None);
    }

    #[test]
    fn f64_edge_values_round_trip_bit_exactly_3288() {
        for bits in F64_EDGE_BITS {
            let v = f64::from_bits(*bits);
            let back: f64 = decode_snapshot_value(&encode_snapshot_value(&v).unwrap()).unwrap();
            assert_eq!(back.to_bits(), *bits, "f64 {bits:#018x} changed");
        }
        let mut source = rich_state();
        source.recalculate_state_root();
        let mut synced = State::default();
        synced.apply_snapshot_entries(&export_state_entries(&source).unwrap()).unwrap();
        for (key, v) in &source.spatial_scores {
            assert_eq!(synced.spatial_scores[key].to_bits(), v.to_bits(), "spatial_scores[{key}]");
        }
        let (e0, e1) = (&source.quality_policy_events[0], &synced.quality_policy_events[0]);
        assert_eq!(e0.scoring_multiplier.to_bits(), e1.scoring_multiplier.to_bits(), "-0.0");
        assert_eq!(e0.reward_multiplier.to_bits(), e1.reward_multiplier.to_bits(), "subnormal");
    }

    #[test]
    fn import_refuses_v1_unknown_missing_and_malformed_entries_3288() {
        let good = export_state_entries(&State::default()).unwrap();
        let without = |key: &str| -> Vec<(Vec<u8>, Vec<u8>)> {
            good.iter().filter(|(k, _)| k != key.as_bytes()).cloned().collect()
        };
        let with = |extra: (&str, Vec<u8>)| -> Vec<(Vec<u8>, Vec<u8>)> {
            let mut v = good.clone();
            v.push((extra.0.as_bytes().to_vec(), extra.1));
            v
        };
        let local = State::default();
        // v1 snapshot: no format version, v1 keys.
        let v1 = vec![(b"oracle/config".to_vec(), b"quorum=1|set=".to_vec())];
        assert!(matches!(import_state_entries(&local, &v1), Err(ApplySnapshotError::UnsupportedEntryType(_))));
        assert!(import_state_entries(&local, &without(FORMAT_VERSION_KEY)).is_err());
        let mut v3 = without(FORMAT_VERSION_KEY);
        v3.push((FORMAT_VERSION_KEY.as_bytes().to_vec(), b"3".to_vec()));
        assert!(import_state_entries(&local, &v3).is_err());
        assert!(import_state_entries(&local, &with(("state/not_a_field", vec![0xf6]))).is_err());
        assert!(import_state_entries(&local, &with(("state/state_root", vec![0xf6]))).is_err(), "node-local field must not be importable");
        assert!(import_state_entries(&local, &without("state/oracle_quorum")).is_err(), "missing field");
        assert!(import_state_entries(&local, &with(("state/oracle_quorum", vec![0x01]))).is_err(), "duplicate field");
        let mut garbage = without("state/oracle_quorum");
        garbage.push((b"state/oracle_quorum".to_vec(), vec![0xff, 0xff]));
        assert!(import_state_entries(&local, &garbage).is_err(), "undecodable value");
        assert!(import_state_entries(&local, &with(("state/oracle_quorum/00", vec![0x01]))).is_err(), "item entry on a non-map field");
        assert!(import_state_entries(&local, &with(("outside/prefix", vec![]))).is_err());
        assert!(import_state_entries(&local, &good).is_ok(), "control: the untouched snapshot imports");
    }

    #[test]
    fn failed_import_leaves_state_untouched_and_runtime_handles_are_kept_3288() {
        let mut local = rich_state();
        local.recalculate_state_root();
        let before = local.state_root;
        let err = local.apply_snapshot_entries(&[(b"garbage".to_vec(), vec![])]);
        assert!(err.is_err());
        assert_eq!(local.state_root, before);
        assert_eq!(first_carried_difference(&local, &rich_state()).unwrap(), None);

        let verifier = local.zk_verifier.clone();
        local.apply_snapshot_entries(&export_state_entries(&State::default()).unwrap()).unwrap();
        assert!(std::sync::Arc::ptr_eq(&verifier, &local.zk_verifier), "runtime zk verifier is the importing node's");
    }
}
