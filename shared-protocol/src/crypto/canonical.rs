//! Canonical encoding with non-canonical rejection (#2124).
//!
//! # Why one byte string per object
//!
//! If two byte strings decode to the same object, a signature over one can be
//! presented alongside the other, and two implementations can hash the same
//! logical object to different digests. That is the root of a large family of
//! consensus splits, and of signature-reuse attacks that need no key material
//! at all — only a re-encoding the verifier accepts.
//!
//! The rule this module enforces is stronger than "decodes successfully":
//!
//! > For every object there is **exactly one** byte string that decodes to it.
//! > Every other byte string is rejected, never normalised.
//!
//! Normalising is the tempting mistake. A decoder that silently accepts an
//! overlong integer and yields the same value has just created a second
//! encoding of one object — which is the property we are trying to eliminate.
//!
//! # The five rejection classes
//!
//! | Variant | Rejects |
//! |---|---|
//! | [`CanonicalError::TrailingBytes`] | anything after a complete object |
//! | [`CanonicalError::NonMinimalInteger`] | overlong varints (`0x80 0x00` for zero) |
//! | [`CanonicalError::DuplicateMapKey`] | a map key appearing twice |
//! | [`CanonicalError::UnorderedMapKey`] | map keys not in strictly ascending order |
//! | [`CanonicalError::NonCanonicalEncoding`] | invalid discriminants, non-canonical NaN, and the re-encode mismatch backstop |
//!
//! # The backstop
//!
//! [`decode_canonical`] does not trust the individual rules to be exhaustive.
//! After decoding it **re-encodes the decoded value and compares byte-for-byte
//! against the input**. Any encoding hazard not caught by a specific rule is
//! still caught here, so a future field type added without a matching rule
//! fails closed rather than opening a second encoding.
//!
//! # Wire format
//!
//! Little-endian LEB128 varints for unsigned integers, zigzag-then-varint for
//! signed, length-prefixed bytes, `0x00`/`0x01` discriminants for `bool` and
//! `Option`, IEEE-754 little-endian bits for `f64` with NaN normalised, and
//! maps as a varint count followed by strictly-ascending key/value pairs.

use std::collections::BTreeMap;
use std::fmt;

/// Why a byte string is not the canonical encoding of any object.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CanonicalError {
    /// A byte at `at` is not a canonical encoding — invalid discriminant,
    /// non-canonical NaN, or (from [`decode_canonical`]) a re-encode mismatch.
    NonCanonicalEncoding { at: usize },
    /// The object decoded completely but `extra` bytes remained.
    TrailingBytes { extra: usize },
    /// A varint used more bytes than the value requires.
    NonMinimalInteger,
    /// The same map key appeared twice.
    DuplicateMapKey,
    /// Map keys were not in strictly ascending order.
    UnorderedMapKey,
    /// Input ended in the middle of a value. Not a canonicality failure, but a
    /// decoder must distinguish it from one.
    UnexpectedEnd,
    /// A non-finite float (NaN or +/-infinity) was offered to the encoder, or
    /// found by the decoder.
    ///
    /// The canonical encoding's value domain is **finite f64 only**. The first
    /// version normalized every NaN to one bit pattern, which made the encoder
    /// non-injective: `f64::from_bits(0x7ff8_0000_0000_0001)` and
    /// `f64::from_bits(0x7ff8_0000_0000_0002)` are distinct Rust values that
    /// produced identical bytes. For an encoding whose purpose is that distinct
    /// values have distinct bytes, silently collapsing 2^52 NaN payloads into
    /// one is the defect the encoding exists to prevent — so the domain is
    /// narrowed and the excluded values are rejected rather than folded.
    NonFiniteFloat,
    /// A length or count exceeded what the remaining input can supply.
    ///
    /// Checked before allocating: a 10-byte message must never be able to ask
    /// for a gigabyte-sized `Vec`.
    LengthOutOfRange { claimed: u64, remaining: usize },
}

impl fmt::Display for CanonicalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonCanonicalEncoding { at } => {
                write!(f, "non-canonical encoding at byte {at}")
            },
            Self::TrailingBytes { extra } => {
                write!(f, "{extra} trailing byte(s) after a complete object")
            },
            Self::NonMinimalInteger => write!(f, "non-minimal integer encoding"),
            Self::DuplicateMapKey => write!(f, "duplicate map key"),
            Self::UnorderedMapKey => write!(f, "map keys not in ascending order"),
            Self::UnexpectedEnd => write!(f, "input ended mid-value"),
            Self::NonFiniteFloat => write!(
                f,
                "non-finite float: the canonical encoding's value domain is finite f64 only"
            ),
            Self::LengthOutOfRange { claimed, remaining } => write!(
                f,
                "claimed length {claimed} exceeds {remaining} remaining byte(s)"
            ),
        }
    }
}

impl std::error::Error for CanonicalError {}

/// An object with exactly one valid byte encoding.
///
/// Implement [`Canonical::write_canonical`] and [`Canonical::read_canonical`];
/// [`Canonical::to_canonical`] and [`Canonical::from_canonical`] are provided
/// and route through [`decode_canonical`], so every implementor gets the
/// trailing-byte check and the re-encode backstop without opting in.
pub trait Canonical: Sized {
    /// Append this object's canonical bytes.
    fn write_canonical(&self, w: &mut CanonicalWriter);

    /// Read one object. Must not check for trailing bytes — the caller does.
    fn read_canonical(r: &mut CanonicalReader<'_>) -> Result<Self, CanonicalError>;

    /// Canonical bytes for this value.
    ///
    /// Fallible because the encodable domain excludes non-finite floats: an
    /// infallible encoder would have to fold them onto some other value, and
    /// folding distinct values onto one byte string is exactly what a canonical
    /// encoding must not do.
    fn to_canonical(&self) -> Result<Vec<u8>, CanonicalError> {
        let mut w = CanonicalWriter::new();
        self.write_canonical(&mut w);
        match w.rejection() {
            Some(e) => Err(e),
            None => Ok(w.finish()),
        }
    }

    fn from_canonical(bytes: &[u8]) -> Result<Self, CanonicalError> {
        decode_canonical(bytes)
    }
}

/// Decode exactly one object from `bytes`, enforcing every canonicality rule.
///
/// Three checks, in order:
/// 1. the type's own decode rules (minimal integers, valid discriminants,
///    ordered unique map keys),
/// 2. no trailing bytes,
/// 3. **re-encoding the decoded value reproduces `bytes` exactly**.
///
/// Step 3 is the backstop described in the module docs. It is what makes the
/// guarantee hold for field types whose specific rule someone forgot to write.
pub fn decode_canonical<T: Canonical>(bytes: &[u8]) -> Result<T, CanonicalError> {
    let mut reader = CanonicalReader::new(bytes);
    let value = T::read_canonical(&mut reader)?;
    reader.expect_end()?;

    // Step 3 also catches a decoded value the encoder would refuse: if
    // re-encoding errors, the input was not something this codec can produce.
    match value.to_canonical() {
        Ok(reencoded) if reencoded == bytes => Ok(value),
        Ok(_) => Err(CanonicalError::NonCanonicalEncoding { at: 0 }),
        Err(e) => Err(e),
    }
}

/// Formerly the single canonical NaN bit pattern.
///
/// Retained only so the constant's history is greppable: NaN is now outside the
/// encodable domain entirely (see [`CanonicalError::NonFiniteFloat`]). The old
/// comment below argued that normalising would give NaN more than one encoding;
/// the actual defect was the reverse — normalising gave *many NaNs* one
/// encoding, so the encoder was not injective. Review of #2234 caught it with a
/// probe over `0x7ff8_0000_0000_0001` and `0x7ff8_0000_0000_0002`.
///
/// Historical note follows: this module
/// exists to prevent.
const CANONICAL_NAN_BITS: u64 = 0x7ff8_0000_0000_0000;

/// Canonical encoder.
#[derive(Default)]
pub struct CanonicalWriter {
    buf: Vec<u8>,
    /// Set when a value outside the encodable domain was offered.
    ///
    /// `write_*` stay infallible so `write_canonical` impls read as a flat list
    /// of fields; the refusal surfaces once, at [`Canonical::to_canonical`].
    rejected: Option<CanonicalError>,
}

impl CanonicalWriter {
    pub fn new() -> Self {
        Self {
            buf: Vec::new(),
            rejected: None,
        }
    }

    /// The rejection recorded during writing, if any.
    pub fn rejection(&self) -> Option<CanonicalError> {
        self.rejected.clone()
    }

    /// Minimal LEB128.
    pub fn write_uint(&mut self, mut value: u64) {
        loop {
            let byte = (value & 0x7f) as u8;
            value >>= 7;
            if value == 0 {
                self.buf.push(byte);
                return;
            }
            self.buf.push(byte | 0x80);
        }
    }

    /// Zigzag, then minimal LEB128, so small negatives stay small.
    pub fn write_int(&mut self, value: i64) {
        self.write_uint(((value << 1) ^ (value >> 63)) as u64);
    }

    pub fn write_u128(&mut self, value: u128) {
        self.buf.extend_from_slice(&value.to_le_bytes());
    }

    pub fn write_bool(&mut self, value: bool) {
        self.buf.push(u8::from(value));
    }

    /// Write a finite `f64` by its exact bit pattern.
    ///
    /// Non-finite values are **rejected**, not normalized — see
    /// [`CanonicalError::NonFiniteFloat`]. Writing the exact bits of a finite
    /// value is injective: distinct finite `f64`s have distinct bit patterns.
    /// (`-0.0` and `0.0` are distinct values with distinct bits and therefore
    /// distinct encodings, which is correct: they are distinguishable in Rust.)
    pub fn write_f64(&mut self, value: f64) {
        if !value.is_finite() {
            self.rejected.get_or_insert(CanonicalError::NonFiniteFloat);
            // Still write something so field offsets stay meaningful for
            // debugging; the buffer is discarded by `to_canonical`.
        }
        self.buf.extend_from_slice(&value.to_bits().to_le_bytes());
    }

    pub fn write_bytes(&mut self, value: &[u8]) {
        self.write_uint(value.len() as u64);
        self.buf.extend_from_slice(value);
    }

    pub fn write_str(&mut self, value: &str) {
        self.write_bytes(value.as_bytes());
    }

    pub fn write_fixed(&mut self, value: &[u8]) {
        self.buf.extend_from_slice(value);
    }

    pub fn write_option<T, F>(&mut self, value: Option<&T>, write: F)
    where
        F: FnOnce(&mut Self, &T),
    {
        match value {
            None => self.buf.push(0),
            Some(v) => {
                self.buf.push(1);
                write(self, v);
            },
        }
    }

    /// Map entries, emitted in strictly ascending key order.
    ///
    /// Collecting into a `BTreeMap` both sorts and deduplicates, so an
    /// unordered source map cannot leak its iteration order into the encoding.
    pub fn write_map<'a, I>(&mut self, entries: I)
    where
        I: IntoIterator<Item = (&'a str, &'a str)>,
    {
        let sorted: BTreeMap<&str, &str> = entries.into_iter().collect();
        self.write_uint(sorted.len() as u64);
        for (k, v) in sorted {
            self.write_str(k);
            self.write_str(v);
        }
    }

    pub fn finish(self) -> Vec<u8> {
        self.buf
    }
}

/// Canonical decoder. Rejects rather than normalises.
pub struct CanonicalReader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> CanonicalReader<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    pub fn position(&self) -> usize {
        self.pos
    }

    pub fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.pos)
    }

    /// Reject any bytes left after a complete object.
    pub fn expect_end(&self) -> Result<(), CanonicalError> {
        if self.remaining() > 0 {
            return Err(CanonicalError::TrailingBytes {
                extra: self.remaining(),
            });
        }
        Ok(())
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], CanonicalError> {
        if self.remaining() < n {
            return Err(CanonicalError::UnexpectedEnd);
        }
        let out = &self.bytes[self.pos..self.pos + n];
        self.pos += n;
        Ok(out)
    }

    fn take_byte(&mut self) -> Result<u8, CanonicalError> {
        Ok(self.take(1)?[0])
    }

    /// LEB128, rejecting any encoding longer than the value needs.
    pub fn read_uint(&mut self) -> Result<u64, CanonicalError> {
        let mut value: u64 = 0;
        let mut shift = 0u32;
        loop {
            let byte = self.take_byte()?;
            if shift >= 64 {
                return Err(CanonicalError::NonMinimalInteger);
            }
            let payload = u64::from(byte & 0x7f);
            // Would this byte's bits fall off the top of a u64?
            if shift == 63 && payload > 1 {
                return Err(CanonicalError::NonMinimalInteger);
            }
            value |= payload << shift;

            if byte & 0x80 == 0 {
                // A continuation that contributed nothing is padding, i.e. an
                // overlong encoding: `0x80 0x00` for zero. Reject, never accept
                // as the same value.
                if shift > 0 && payload == 0 {
                    return Err(CanonicalError::NonMinimalInteger);
                }
                return Ok(value);
            }
            shift += 7;
        }
    }

    pub fn read_int(&mut self) -> Result<i64, CanonicalError> {
        let raw = self.read_uint()?;
        Ok(((raw >> 1) as i64) ^ -((raw & 1) as i64))
    }

    pub fn read_u128(&mut self) -> Result<u128, CanonicalError> {
        let bytes = self.take(16)?;
        let mut buf = [0u8; 16];
        buf.copy_from_slice(bytes);
        Ok(u128::from_le_bytes(buf))
    }

    pub fn read_bool(&mut self) -> Result<bool, CanonicalError> {
        let at = self.pos;
        match self.take_byte()? {
            0 => Ok(false),
            1 => Ok(true),
            // Anything else is a second encoding of `true` if accepted.
            _ => Err(CanonicalError::NonCanonicalEncoding { at }),
        }
    }

    pub fn read_f64(&mut self) -> Result<f64, CanonicalError> {
        let at = self.pos;
        let bytes = self.take(8)?;
        let mut buf = [0u8; 8];
        buf.copy_from_slice(bytes);
        let bits = u64::from_le_bytes(buf);
        let value = f64::from_bits(bits);
        // The encoder cannot produce a non-finite float, so accepting one on
        // decode would accept bytes no honest encoder emits — and would break
        // the decode-then-re-encode fixed-point property `decode_canonical`
        // enforces.
        if !value.is_finite() {
            let _ = at;
            return Err(CanonicalError::NonFiniteFloat);
        }
        Ok(value)
    }

    pub fn read_bytes(&mut self) -> Result<Vec<u8>, CanonicalError> {
        let len = self.read_uint()?;
        // Bound the allocation against the input, not against the claim.
        if len > self.remaining() as u64 {
            return Err(CanonicalError::LengthOutOfRange {
                claimed: len,
                remaining: self.remaining(),
            });
        }
        Ok(self.take(len as usize)?.to_vec())
    }

    pub fn read_string(&mut self) -> Result<String, CanonicalError> {
        let at = self.pos;
        let bytes = self.read_bytes()?;
        String::from_utf8(bytes).map_err(|_| CanonicalError::NonCanonicalEncoding { at })
    }

    pub fn read_fixed<const N: usize>(&mut self) -> Result<[u8; N], CanonicalError> {
        let bytes = self.take(N)?;
        let mut buf = [0u8; N];
        buf.copy_from_slice(bytes);
        Ok(buf)
    }

    pub fn read_option<T, F>(&mut self, read: F) -> Result<Option<T>, CanonicalError>
    where
        F: FnOnce(&mut Self) -> Result<T, CanonicalError>,
    {
        let at = self.pos;
        match self.take_byte()? {
            0 => Ok(None),
            1 => Ok(Some(read(self)?)),
            _ => Err(CanonicalError::NonCanonicalEncoding { at }),
        }
    }

    /// Read a map, requiring strictly ascending unique keys.
    ///
    /// Duplicate and unordered are reported distinctly: "the encoder sorted but
    /// repeated" and "the encoder did not sort" are different bugs.
    pub fn read_map(&mut self) -> Result<BTreeMap<String, String>, CanonicalError> {
        let count = self.read_uint()?;
        // Two length-prefixed fields per entry, so at least 2 bytes each.
        if count > (self.remaining() as u64) / 2 {
            return Err(CanonicalError::LengthOutOfRange {
                claimed: count,
                remaining: self.remaining(),
            });
        }

        let mut out = BTreeMap::new();
        let mut previous: Option<String> = None;
        for _ in 0..count {
            let key = self.read_string()?;
            let value = self.read_string()?;
            if let Some(prev) = &previous {
                match key.as_str().cmp(prev.as_str()) {
                    std::cmp::Ordering::Equal => return Err(CanonicalError::DuplicateMapKey),
                    std::cmp::Ordering::Less => return Err(CanonicalError::UnorderedMapKey),
                    std::cmp::Ordering::Greater => {},
                }
            }
            previous = Some(key.clone());
            out.insert(key, value);
        }
        Ok(out)
    }
}

/// Conformance vector type for the canonical codec.
///
/// Exercises every primitive the codec supports, in one object. This is what
/// the negative fixtures under `shared-protocol/tests/fixtures/canonical/`
/// encode and corrupt, and what fuzz targets should drive — a fuzzer reaching
/// every primitive through one entry point beats one target per real type.
///
/// It is deliberately `pub` and not `#[cfg(test)]`: fixtures that only tests
/// can parse are not reusable by the fuzz targets the story requires.
#[derive(Clone, Debug, PartialEq)]
pub struct CanonicalProbe {
    pub count: u64,
    pub offset: i64,
    pub weight: u128,
    pub flag: bool,
    pub score: f64,
    pub label: String,
    pub digest: [u8; 4],
    pub note: Option<String>,
    pub attributes: BTreeMap<String, String>,
}

impl Canonical for CanonicalProbe {
    fn write_canonical(&self, w: &mut CanonicalWriter) {
        w.write_uint(self.count);
        w.write_int(self.offset);
        w.write_u128(self.weight);
        w.write_bool(self.flag);
        w.write_f64(self.score);
        w.write_str(&self.label);
        w.write_fixed(&self.digest);
        w.write_option(self.note.as_ref(), |w, v| w.write_str(v));
        w.write_map(
            self.attributes
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str())),
        );
    }

    fn read_canonical(r: &mut CanonicalReader<'_>) -> Result<Self, CanonicalError> {
        Ok(Self {
            count: r.read_uint()?,
            offset: r.read_int()?,
            weight: r.read_u128()?,
            flag: r.read_bool()?,
            score: r.read_f64()?,
            label: r.read_string()?,
            digest: r.read_fixed::<4>()?,
            note: r.read_option(|r| r.read_string())?,
            attributes: r.read_map()?,
        })
    }
}

// ---------------------------------------------------------------------------
// Implementations for signed protocol objects
// ---------------------------------------------------------------------------
//
// Same object list as the domain-separation story (#2122), so a signed object
// has one encoding and one domain tag. These are additive: no existing
// serialization path changes.

use crate::blockchain::snapshot_sync::SnapshotSyncPayload;
use crate::blockchain::transaction::{GovernanceVotePayload, OracleAttestationPayload};
use crate::cross_chain::types::{
    BridgeMessage, BridgeMessageType, CrossChainDirection, CrossChainNetwork,
};
use crate::security::slashing::SlashingReportPayload;
use crate::weather::validation::WeatherReading;

impl Canonical for SnapshotSyncPayload {
    fn write_canonical(&self, w: &mut CanonicalWriter) {
        w.write_uint(self.height);
        w.write_str(&self.state_root_hex);
        w.write_str(&self.snapshot_root_hex);
        w.write_uint(self.entry_count);
        w.write_str(&self.tip_hash_hex);
        w.write_uint(self.finalized_height);
    }

    fn read_canonical(r: &mut CanonicalReader<'_>) -> Result<Self, CanonicalError> {
        Ok(Self {
            height: r.read_uint()?,
            state_root_hex: r.read_string()?,
            snapshot_root_hex: r.read_string()?,
            entry_count: r.read_uint()?,
            tip_hash_hex: r.read_string()?,
            finalized_height: r.read_uint()?,
        })
    }
}

impl Canonical for WeatherReading {
    fn write_canonical(&self, w: &mut CanonicalWriter) {
        w.write_int(self.timestamp_unix);
        w.write_option(self.temperature_c.as_ref(), |w, v| w.write_f64(*v));
        w.write_option(self.pressure_hpa.as_ref(), |w, v| w.write_f64(*v));
        w.write_option(self.humidity_pct.as_ref(), |w, v| w.write_f64(*v));
        w.write_option(self.wind_speed_ms.as_ref(), |w, v| w.write_f64(*v));
    }

    fn read_canonical(r: &mut CanonicalReader<'_>) -> Result<Self, CanonicalError> {
        Ok(Self {
            timestamp_unix: r.read_int()?,
            temperature_c: r.read_option(|r| r.read_f64())?,
            pressure_hpa: r.read_option(|r| r.read_f64())?,
            humidity_pct: r.read_option(|r| r.read_f64())?,
            wind_speed_ms: r.read_option(|r| r.read_f64())?,
        })
    }
}

// `FinalitySignal` deliberately has NO `Canonical` impl.
//
// It is not signed — #2231 removed it from domain coverage for exactly that
// reason — and its `source` field is a `&'static str` observability label. The
// previous impl omitted `source` and the decoder substituted the literal
// `"canonical"`, so two signals differing only in `source` shared a preimage and
// a round-trip was not the identity.
//
// For an unsigned type that is harmless today and a forgery surface the moment
// somebody signs it. Removing the impl removes the claim: there is no canonical
// encoding of a `FinalitySignal` to misuse. If one is ever needed, encode
// `source` and give it a domain tag in the #2231 registry at the same time.

impl Canonical for OracleAttestationPayload {
    fn write_canonical(&self, w: &mut CanonicalWriter) {
        w.write_str(&self.commit_id);
        w.write_str(&self.oracle_id);
        w.write_bool(self.approved);
        w.write_option(self.signature.as_ref(), |w, v| w.write_str(v));
        w.write_option(self.confidence.as_ref(), |w, v| w.write_f64(*v));
        w.write_int(self.timestamp);
        w.write_option(self.nonce.as_ref(), |w, v| w.write_str(v));
    }

    fn read_canonical(r: &mut CanonicalReader<'_>) -> Result<Self, CanonicalError> {
        Ok(Self {
            commit_id: r.read_string()?,
            oracle_id: r.read_string()?,
            approved: r.read_bool()?,
            signature: r.read_option(|r| r.read_string())?,
            confidence: r.read_option(|r| r.read_f64())?,
            timestamp: r.read_int()?,
            nonce: r.read_option(|r| r.read_string())?,
        })
    }
}

impl Canonical for GovernanceVotePayload {
    fn write_canonical(&self, w: &mut CanonicalWriter) {
        w.write_str(&self.proposal_id);
        w.write_bool(self.approve);
        w.write_option(self.voting_power.as_ref(), |w, v| w.write_uint(*v));
        w.write_option(self.signature.as_ref(), |w, v| w.write_str(v));
        w.write_option(self.signer_public_key.as_ref(), |w, v| w.write_str(v));
    }

    fn read_canonical(r: &mut CanonicalReader<'_>) -> Result<Self, CanonicalError> {
        Ok(Self {
            proposal_id: r.read_string()?,
            approve: r.read_bool()?,
            voting_power: r.read_option(|r| r.read_uint())?,
            signature: r.read_option(|r| r.read_string())?,
            signer_public_key: r.read_option(|r| r.read_string())?,
        })
    }
}

impl Canonical for SlashingReportPayload {
    fn write_canonical(&self, w: &mut CanonicalWriter) {
        w.write_str(&self.validator_id);
        w.write_uint(u64::from(slashing_evidence_code(&self.evidence_type)));
        w.write_fixed(&self.evidence_hash);
        w.write_uint(self.reported_at_height);
        w.write_u128(self.penalty_amount);
        w.write_str(&self.reporter_pubkey);
        w.write_option(self.reporter_signature.as_ref(), |w, v| w.write_str(v));
    }

    fn read_canonical(r: &mut CanonicalReader<'_>) -> Result<Self, CanonicalError> {
        let validator_id = r.read_string()?;
        let at = r.position();
        let code = r.read_uint()?;
        let evidence_type =
            slashing_evidence_from_code(code).ok_or(CanonicalError::NonCanonicalEncoding { at })?;
        Ok(Self {
            validator_id,
            evidence_type,
            evidence_hash: r.read_fixed::<32>()?,
            reported_at_height: r.read_uint()?,
            penalty_amount: r.read_u128()?,
            reporter_pubkey: r.read_string()?,
            reporter_signature: r.read_option(|r| r.read_string())?,
        })
    }
}

/// Stable numeric tag for a cross-chain network.
///
/// A tag rather than `format!("{:?}")`: renaming a variant must not silently
/// change an encoding that signatures commit to. `Custom` carries its label as a
/// separate length-prefixed string, so `Custom("ethereum")` and `Ethereum`
/// encode differently — `as_id()` would have collapsed them, which is precisely
/// the kind of collision a canonical encoding exists to prevent.
fn write_network(w: &mut CanonicalWriter, n: &CrossChainNetwork) {
    use CrossChainNetwork as N;
    let tag: u64 = match n {
        N::Custom(_) => 0,
        N::Ethereum => 1,
        N::Polygon => 2,
        N::Avalanche => 3,
        N::Arbitrum => 4,
        N::Optimism => 5,
        N::Base => 6,
        N::Solana => 7,
        N::Sui => 8,
        N::Aptos => 9,
        N::Cosmos => 10,
        N::Polkadot => 11,
    };
    w.write_uint(tag);
    if let N::Custom(label) = n {
        w.write_str(label);
    }
}

fn read_network(r: &mut CanonicalReader<'_>) -> Result<CrossChainNetwork, CanonicalError> {
    use CrossChainNetwork as N;
    let at = r.position();
    let tag = r.read_uint()?;
    Ok(match tag {
        0 => N::Custom(r.read_string()?),
        1 => N::Ethereum,
        2 => N::Polygon,
        3 => N::Avalanche,
        4 => N::Arbitrum,
        5 => N::Optimism,
        6 => N::Base,
        7 => N::Solana,
        8 => N::Sui,
        9 => N::Aptos,
        10 => N::Cosmos,
        11 => N::Polkadot,
        _ => return Err(CanonicalError::NonCanonicalEncoding { at }),
    })
}

/// Canonical form of an arbitrary JSON payload.
///
/// `serde_json::Value`'s object type is a `BTreeMap` in this workspace
/// (`preserve_order` is not enabled), so `to_string` emits keys in sorted order
/// with no insignificant whitespace, and numbers through a deterministic
/// formatter. That makes serialization a deterministic, injective function of
/// the `Value`, which is what a signature needs.
///
/// Encoding the payload at all is the point: omitting it, as the first version
/// did, let two bridge messages differing only in `payload` share a preimage —
/// so a signature over one authenticated the other.
fn write_json(w: &mut CanonicalWriter, v: Option<&serde_json::Value>) {
    w.write_option(v, |w, v| w.write_str(&v.to_string()));
}

fn read_json(r: &mut CanonicalReader<'_>) -> Result<Option<serde_json::Value>, CanonicalError> {
    let at = r.position();
    match r.read_option(|r| r.read_string())? {
        None => Ok(None),
        Some(raw) => serde_json::from_str(&raw)
            .map(Some)
            .map_err(|_| CanonicalError::NonCanonicalEncoding { at }),
    }
}

impl Canonical for BridgeMessage {
    fn write_canonical(&self, w: &mut CanonicalWriter) {
        w.write_str(&self.message_id);
        write_network(w, &self.source_network);
        write_network(w, &self.target_network);
        w.write_uint(match self.direction {
            CrossChainDirection::Inbound => 0,
            CrossChainDirection::Outbound => 1,
        });
        w.write_uint(match self.message_type {
            BridgeMessageType::Lock => 0,
            BridgeMessageType::Unlock => 1,
            BridgeMessageType::Mint => 2,
            BridgeMessageType::Burn => 3,
            BridgeMessageType::Attestation => 4,
        });
        w.write_str(&self.sender);
        w.write_str(&self.recipient);
        w.write_uint(self.amount);
        w.write_uint(self.nonce);
        w.write_int(self.timestamp);
        w.write_option(self.tx_hash.as_ref(), |w, v| w.write_str(v));
        write_json(w, self.payload.as_ref());
    }

    fn read_canonical(r: &mut CanonicalReader<'_>) -> Result<Self, CanonicalError> {
        let message_id = r.read_string()?;
        let source_network = read_network(r)?;
        let target_network = read_network(r)?;

        let at = r.position();
        let direction = match r.read_uint()? {
            0 => CrossChainDirection::Inbound,
            1 => CrossChainDirection::Outbound,
            _ => return Err(CanonicalError::NonCanonicalEncoding { at }),
        };

        let at = r.position();
        let message_type = match r.read_uint()? {
            0 => BridgeMessageType::Lock,
            1 => BridgeMessageType::Unlock,
            2 => BridgeMessageType::Mint,
            3 => BridgeMessageType::Burn,
            4 => BridgeMessageType::Attestation,
            _ => return Err(CanonicalError::NonCanonicalEncoding { at }),
        };

        Ok(Self {
            message_id,
            source_network,
            target_network,
            direction,
            message_type,
            sender: r.read_string()?,
            recipient: r.read_string()?,
            amount: r.read_uint()?,
            nonce: r.read_uint()?,
            timestamp: r.read_int()?,
            tx_hash: r.read_option(|r| r.read_string())?,
            payload: read_json(r)?,
        })
    }
}

/// Stable numeric code for a slashing evidence type.
///
/// A numeric code rather than the `Debug` string: renaming a variant must not
/// silently change an encoding that signatures depend on.
fn slashing_evidence_code(evidence: &crate::security::slashing::SlashingEvidenceType) -> u32 {
    use crate::security::slashing::SlashingEvidenceType as E;
    match evidence {
        E::DoubleSign => 1,
        E::InvalidZkProof => 2,
        E::RepeatedLowQualityData => 3,
    }
}

fn slashing_evidence_from_code(
    code: u64,
) -> Option<crate::security::slashing::SlashingEvidenceType> {
    use crate::security::slashing::SlashingEvidenceType as E;
    match code {
        1 => Some(E::DoubleSign),
        2 => Some(E::InvalidZkProof),
        3 => Some(E::RepeatedLowQualityData),
        _ => None,
    }
}
