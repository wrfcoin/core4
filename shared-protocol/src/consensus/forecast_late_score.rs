//! Forecast late score (`ConsensusFeature::ForecastLateScore`).
//!
//! A forecast is revealed within a few blocks of its commit, but the truth it
//! predicts is observed hours later. This module is the lane that scores a
//! forecast once the truth is known and pays for skill:
//!
//! 1. **Record.** When a forecast reveal applies, the chain stores a
//!    [`ForecastRecord`]: the forecaster (the key that signed the commit), the
//!    station, one integer value, the lead, and the commit block's height and
//!    time. The target time is `commit time + lead`; it comes from the chain.
//! 2. **Score.** A `TransactionType::ForecastScore` transaction carries an
//!    [`ObservationAttestation`] signed by an oracle quorum, plus the commit
//!    ids to score. Anyone may relay it. The chain computes the skill with
//!    the integer rule in `wrfcoin_forecast_score::late_score` (rule 7 of the
//!    shared skill vectors); nothing is taken from the transaction except the
//!    oracle-signed observation.
//! 3. **Close and pay.** A bucket (station, variable, lead, target hour)
//!    closes 24 h after the end of its hour. Its top forecasters by skill are
//!    paid from a capped share of the Contribution slice. Whatever is not paid
//!    is not minted. Unscored forecasts are dropped unpaid.
//!
//! What the chain checks of an observation, and what it leaves to the oracles
//! that sign it:
//!
//! - On chain: the quorum of authorized oracle signatures; the range rule; the
//!   timing (not before target + 35 min); that the truth reading and every
//!   window reading cited is an on-chain reading of the station at oracle
//!   quorum; and that every window reading LANDED below the forecast's commit
//!   height, so a reading back-dated after the forecast was revealed can never
//!   enter the window or become `last`.
//! - Oracle side: which readings fall in the 24 h window and the 15-minute
//!   truth window by block time and observation time, quorum as of the closing
//!   block, the provenance class, the hourly samples, minimum sample counts,
//!   and the reference arithmetic. The chain never parses or rounds a decimal
//!   or a float: readings reach it only as the integers oracles sign.
//!
//! Everything is integer arithmetic over `BTreeMap`s, with checked operations;
//! a failed check refuses the step (fail closed). Before the feature is in
//! force nothing here is written, so [`ForecastLateScoreState`] is empty and
//! absent from the state root, checkpoints and snapshots.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sha3::{Digest, Sha3_256};
use thiserror::Error;
pub use wrfcoin_forecast_score::late_score::ScoredVariable as LateScoreVariable;
use wrfcoin_forecast_score::late_score::{
    DEFAULT_COPY_TOLERANCE_UNITS, LateScoreInput, MAX_REFERENCE, SCORE_DELAY_SECS, SKILL_PRECISION,
    late_skill,
};

use crate::blockchain::transaction::{RevealPayload, Transaction, TransactionType};
use crate::common::constants::WRF_UNITS_PER_COIN;
use crate::common::types::{Address, Balance};
use crate::security::key_registry::{SigningKeyRegistry, SigningRole};

/// Domain tag of what oracles sign for a score: the full score payload (the
/// observation and the forecast commit ids it scores), see
/// [`forecast_score_message`].
pub const FORECAST_SCORE_ATTESTATION_DOMAIN_TAG: &[u8] =
    b"wrfcoin.forecast_score_attestation.v2";
/// Domain tag of the input outpoint a score transaction must spend
/// ([`score_outpoint`]).
pub const FORECAST_SCORE_OUTPOINT_DOMAIN_TAG: &[u8] = b"wrfcoin.forecast_score_outpoint.v1";
/// Domain tag of the observation digest that pins a commit block's forecasts
/// to one observation ([`observation_digest`]). Never signed.
const OBSERVATION_DIGEST_DOMAIN_TAG: &[u8] = b"wrfcoin.forecast_observation_digest.v1";
/// Domain tag of a bucket id.
const BUCKET_ID_DOMAIN_TAG: &[u8] = b"wrfcoin.forecast_late_score.bucket.v1";
/// Domain tag of the state-root section.
const ROOT_MATERIAL_TAG: &[u8] = b"wrfcoin_forecast_late_score_v1";

/// Largest `Transaction::data` of a score transaction.
pub const MAX_FORECAST_SCORE_PAYLOAD_BYTES: usize = 64 * 1024;
/// Most forecast commit ids one score transaction may list.
pub const MAX_FORECASTS_PER_SCORE_TX: usize = 64;
/// Most score transactions one block may carry.
pub const MAX_FORECAST_SCORE_TXS_PER_BLOCK: usize = 8;
/// Most window-reading commit ids one attestation may cite. A station with
/// more quorum readings than this in 24 h cannot be attested (unscored).
pub const MAX_WINDOW_READINGS: usize = 512;
/// Most oracle signatures one score transaction may carry.
pub const MAX_OBSERVATION_SIGNATURES: usize = 16;
/// Longest station id or commit id accepted in a score transaction.
pub const MAX_ID_BYTES: usize = 128;

/// The leads a forecast may be scored at, in hours.
pub const LEAD_HOURS: [u32; 4] = [1, 6, 12, 24];
/// A forecast may be scored from its target time plus this many seconds:
/// 15 minutes of readings plus 20 minutes for them to land and reach quorum.
pub const SCORE_OPENS_AFTER_SECS: i64 = SCORE_DELAY_SECS;
/// A bucket closes, and pays, this many seconds after the end of its hour.
pub const SCORE_WINDOW_SECS: i64 = 24 * 60 * 60;
const SECS_PER_HOUR: i64 = 3_600;

/// Most forecasts one bucket holds; later reveals for a full bucket are not
/// recorded.
pub const MAX_BUCKET_ENTRIES: usize = 32;
/// Forecasters paid per bucket.
pub const PAID_PER_BUCKET: usize = 3;
/// Most buckets closed in one block; the rest close in the following blocks.
pub const MAX_BUCKET_CLOSES_PER_BLOCK: usize = 256;

/// Pay for a perfect forecast before caps: `nominal = skill × SKILL_UNIT / 10^6`.
pub const SKILL_UNIT: u128 = WRF_UNITS_PER_COIN as u128;
/// Most one bucket pays in total.
pub const BUCKET_CAP: u128 = 2 * SKILL_UNIT;
/// Length of a forecast pool epoch, in blocks.
pub const POOL_EPOCH_BLOCKS: u64 = 1_000;
/// The epoch budget, as a share of the Contribution slice over the epoch.
pub const POOL_SHARE_BPS: u128 = 500;
/// Most the lane may take of one block's Contribution slice.
pub const BLOCK_SHARE_BPS: u128 = 5_000;
/// Most one forecaster may earn in an epoch, as a share of the epoch budget.
pub const FORECASTER_EPOCH_SHARE_BPS: u128 = 1_000;
/// Pool epochs kept in state after they end.
pub const POOL_EPOCHS_RETAINED: u64 = 4;
const BPS: u128 = 10_000;

/// The copy tolerance, in the variable's fixed-point unit (0.05 °C; 5 Pa).
/// One named constant for the baseline copy check and the copier rule.
pub const COPY_TOLERANCE_UNITS: u64 = DEFAULT_COPY_TOLERANCE_UNITS;

/// The copy tolerance of `variable`.
pub const fn copy_tolerance(_variable: LateScoreVariable) -> u64 {
    COPY_TOLERANCE_UNITS
}

/// The key of the integer value in a forecast reveal's `data`:
/// `temp_c_centi` (0.01 °C) or `pressure_pa` (Pa).
pub const fn data_key(variable: LateScoreVariable) -> &'static str {
    match variable {
        LateScoreVariable::TemperatureC => "temp_c_centi",
        LateScoreVariable::PressureHpa => "pressure_pa",
    }
}

/// Stable byte of a variable in signed and hashed encodings.
const fn wire_byte(variable: LateScoreVariable) -> u8 {
    match variable {
        LateScoreVariable::TemperatureC => 0,
        LateScoreVariable::PressureHpa => 1,
    }
}

/// The observation that the forecasts of one station, variable and lead
/// COMMITTED IN ONE BLOCK are scored against. An oracle quorum signs it
/// together with the forecast ids it scores ([`forecast_score_message`]). The
/// window and `last` depend on the commit block, so there is one attestation
/// per commit block. Every number is a fixed-point integer in the variable's
/// unit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservationAttestation {
    pub station_id: String,
    pub variable: LateScoreVariable,
    /// Lead in hours.
    pub lead_hours: u32,
    /// Height and time (unix seconds) of the block the forecasts were
    /// committed in.
    pub commit_height: u64,
    pub commit_time: i64,
    /// `commit_time + lead_hours`, the time scored.
    pub target: i64,
    /// The station reading nearest `target`.
    pub truth: i64,
    /// Commit id of that reading.
    pub truth_commit_id: String,
    /// The smaller of the window's spread and its lagged-persistence error.
    pub reference: u64,
    /// Mean of the window's hourly samples.
    pub mean24: i64,
    /// The last reading before the commit.
    pub last: i64,
    /// Lowercase hex SHA3-256 of the 24 hourly samples
    /// (`wrfcoin_forecast_score::late_score::window_hash`).
    pub window_hash: String,
    /// Commit ids of the readings the window and `last` were built from,
    /// strictly ascending in byte order. Every one landed below
    /// `commit_height`.
    pub window_commit_ids: Vec<String>,
}

/// One oracle's signature over [`forecast_score_message`]: the full score
/// payload except the signatures.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservationSignature {
    /// The oracle's identity, exactly as the chain's oracle set and signing-key
    /// registry hold it. The key is looked up in the registry; this string is
    /// never used as a key.
    pub oracle: String,
    /// Hex ed25519 signature.
    pub ed25519: String,
}

/// `Transaction::data` of a `TransactionType::ForecastScore` transaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ForecastScorePayload {
    pub attestation: ObservationAttestation,
    /// Oracle signatures over [`forecast_score_message`] (the attestation and
    /// `forecast_commit_ids`), strictly ascending by oracle.
    pub signatures: Vec<ObservationSignature>,
    /// Forecast commit ids to score, strictly ascending.
    pub forecast_commit_ids: Vec<String>,
}

/// Why a score transaction, or a step of the lane, is refused.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ForecastLateScoreError {
    #[error("malformed score payload: {0}")]
    Malformed(&'static str),
    #[error("oracle signature does not verify")]
    BadSignature,
    #[error("signer is not an authorized oracle")]
    UnauthorizedOracle,
    #[error("{have} authorized oracle signatures, quorum is {need}")]
    BelowQuorum { have: usize, need: usize },
    #[error("too early: the forecast can be scored from {opens_at}")]
    TooEarly { opens_at: i64 },
    #[error("too late: the bucket closed at {closes_at}")]
    Closed { closes_at: i64 },
    #[error("no forecast is recorded for this station, variable, lead and hour")]
    UnknownBucket,
    #[error("these forecasts are already scored against a different observation")]
    ObservationConflict,
    #[error("the truth reading is not an attested on-chain reading of this station")]
    TruthNotAttested,
    #[error("a window reading is not an attested on-chain reading of this station")]
    ReadingNotAttested,
    #[error("a window reading landed at or after the forecast's commit height")]
    ReadingAfterCommit,
    #[error("a cited reading is of the synthetic provenance class")]
    SyntheticReading,
    #[error(
        "the truth reading did not land after the forecast's commit height, or is also cited in the window"
    )]
    TruthNotAfterCommit,
    #[error(
        "forecast {0} does not belong to the attested station, variable, lead and commit block"
    )]
    ForecastNotInBucket(String),
    #[error("no listed forecast is waiting for a score")]
    NothingToScore,
    #[error("forecast {0} is already scored")]
    AlreadyScored(String),
    #[error("score arithmetic refused: {0}")]
    Score(String),
    #[error("arithmetic overflow")]
    Overflow,
}

fn push(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
    out.extend_from_slice(bytes);
}

fn is_lower_hex(s: &str, len: usize) -> bool {
    s.len() == len
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn is_plain_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= MAX_ID_BYTES && s.bytes().all(|b| b.is_ascii_graphic())
}

/// Every field of an observation, in a stable, length-prefixed, non-serde
/// encoding: a change to the struct layout can never silently change signed
/// or hashed bytes.
fn push_observation(message: &mut Vec<u8>, attestation: &ObservationAttestation) {
    push(message, attestation.station_id.as_bytes());
    push(message, &[wire_byte(attestation.variable)]);
    push(message, &attestation.lead_hours.to_le_bytes());
    push(message, &attestation.commit_height.to_le_bytes());
    push(message, &attestation.commit_time.to_le_bytes());
    push(message, &attestation.target.to_le_bytes());
    push(message, &attestation.truth.to_le_bytes());
    push(message, attestation.truth_commit_id.as_bytes());
    push(message, &attestation.reference.to_le_bytes());
    push(message, &attestation.mean24.to_le_bytes());
    push(message, &attestation.last.to_le_bytes());
    push(message, attestation.window_hash.as_bytes());
    push(message, &(attestation.window_commit_ids.len() as u64).to_le_bytes());
    for id in &attestation.window_commit_ids {
        push(message, id.as_bytes());
    }
}

/// The bytes an oracle signs for a score on this chain: the domain tag, the
/// chain identity, every field of the observation, then the forecast commit
/// ids it scores (count, then each id, length-prefixed, in the payload's
/// strictly ascending order). The signature covers the full score payload
/// except the signatures, so it binds exactly what apply reads.
pub fn forecast_score_message(
    chain_id: &str,
    network_id: &str,
    attestation: &ObservationAttestation,
    forecast_commit_ids: &[String],
) -> Vec<u8> {
    let mut message = Vec::new();
    push(&mut message, FORECAST_SCORE_ATTESTATION_DOMAIN_TAG);
    push(&mut message, chain_id.as_bytes());
    push(&mut message, network_id.as_bytes());
    push_observation(&mut message, attestation);
    push(&mut message, &(forecast_commit_ids.len() as u64).to_le_bytes());
    for id in forecast_commit_ids {
        push(&mut message, id.as_bytes());
    }
    message
}

/// The one input outpoint (txid, vout 0) a `ForecastScore` transaction
/// spends: derived from the signed score message, so every transaction
/// carrying the same signed payload spends the same outpoint and the mempool
/// holds at most one of them. Mempool admission enforces it; apply does not
/// read it.
pub fn score_outpoint(
    chain_id: &str,
    network_id: &str,
    payload: &ForecastScorePayload,
) -> crate::crypto::hash::Hash {
    let mut material = Vec::new();
    push(&mut material, FORECAST_SCORE_OUTPOINT_DOMAIN_TAG);
    push(
        &mut material,
        &forecast_score_message(
            chain_id,
            network_id,
            &payload.attestation,
            &payload.forecast_commit_ids,
        ),
    );
    crate::crypto::hash::hash_bytes(&material)
}

/// Lowercase hex SHA3-256 of the observation alone (its own domain tag, the
/// chain identity, every observation field): what the forecasts of one
/// commit block are pinned to once scored. It leaves out the forecast ids, so
/// two scores of the same commit block against the same observation (for
/// different forecasts) agree on it. Never signed.
pub fn observation_digest(
    chain_id: &str,
    network_id: &str,
    attestation: &ObservationAttestation,
) -> String {
    let mut material = Vec::new();
    push(&mut material, OBSERVATION_DIGEST_DOMAIN_TAG);
    push(&mut material, chain_id.as_bytes());
    push(&mut material, network_id.as_bytes());
    push_observation(&mut material, attestation);
    hex::encode(Sha3_256::digest(material))
}

/// The time a forecast committed at `commit_time` with `lead_hours` is scored
/// at: `commit_time + lead`. `None` for an unsupported lead or a negative or
/// overflowing time.
pub fn target_for(commit_time: i64, lead_hours: u32) -> Option<i64> {
    if commit_time < 0 || !LEAD_HOURS.contains(&lead_hours) {
        return None;
    }
    commit_time.checked_add(i64::from(lead_hours).checked_mul(SECS_PER_HOUR)?)
}

/// The start of the hour `target` falls in: the hour a bucket competes over.
pub fn bucket_hour(target: i64) -> Option<i64> {
    (target >= 0).then(|| target - target % SECS_PER_HOUR)
}

/// The id of the bucket (station, variable, lead, target hour). It sorts by
/// hour first, so the buckets due to close are a prefix of the map.
pub fn bucket_id(
    station_id: &str,
    variable: LateScoreVariable,
    lead_hours: u32,
    hour: i64,
) -> Option<String> {
    let sortable = u64::try_from(hour).ok()?;
    let mut material = Vec::new();
    push(&mut material, BUCKET_ID_DOMAIN_TAG);
    push(&mut material, station_id.as_bytes());
    push(&mut material, &[wire_byte(variable)]);
    push(&mut material, &lead_hours.to_le_bytes());
    push(&mut material, &hour.to_le_bytes());
    let digest = Sha3_256::digest(&material);
    Some(format!("{sortable:016x}-{}", hex::encode(&digest[..16])))
}

impl ObservationAttestation {
    /// Stateless shape and range checks.
    pub fn validate(&self) -> Result<(), ForecastLateScoreError> {
        use ForecastLateScoreError::Malformed;
        if !is_plain_id(&self.station_id) {
            return Err(Malformed("station_id"));
        }
        if target_for(self.commit_time, self.lead_hours) != Some(self.target) {
            return Err(Malformed(
                "target is not commit_time + a lead of 1, 6, 12 or 24 hours",
            ));
        }
        // Rule 1: every reading-derived value is inside the variable's range.
        if !(self.variable.in_range(self.truth)
            && self.variable.in_range(self.mean24)
            && self.variable.in_range(self.last))
        {
            return Err(Malformed("truth, mean24 or last is out of range"));
        }
        if self.reference > MAX_REFERENCE {
            return Err(Malformed("reference is out of range"));
        }
        if !is_lower_hex(&self.window_hash, 64) {
            return Err(Malformed("window_hash is not 64 lowercase hex characters"));
        }
        if !is_plain_id(&self.truth_commit_id) {
            return Err(Malformed("truth_commit_id"));
        }
        if self.window_commit_ids.is_empty() || self.window_commit_ids.len() > MAX_WINDOW_READINGS {
            return Err(Malformed("window_commit_ids count"));
        }
        if !self.window_commit_ids.iter().all(|id| is_plain_id(id))
            || !self
                .window_commit_ids
                .windows(2)
                .all(|pair| pair[0].as_bytes() < pair[1].as_bytes())
        {
            return Err(Malformed(
                "window_commit_ids must be plain ids, strictly ascending",
            ));
        }
        Ok(())
    }
}

impl ForecastScorePayload {
    /// Parse a `ForecastScore` transaction. `None` for any other type, an
    /// oversized payload or one that is not exactly this JSON shape.
    pub fn parse(tx: &Transaction) -> Option<Self> {
        if tx.tx_type != TransactionType::ForecastScore {
            return None;
        }
        let data = tx.data.as_ref()?;
        if data.len() > MAX_FORECAST_SCORE_PAYLOAD_BYTES {
            return None;
        }
        serde_json::from_str(data).ok()
    }

    /// Stateless shape and range checks of the whole payload. No signature is
    /// checked here: an oracle's key is whatever the chain's signing-key
    /// registry says it is at the time, which is a state question.
    pub fn validate(&self) -> Result<(), ForecastLateScoreError> {
        use ForecastLateScoreError::Malformed;
        self.attestation.validate()?;
        if self.signatures.is_empty() || self.signatures.len() > MAX_OBSERVATION_SIGNATURES {
            return Err(Malformed("signature count"));
        }
        // Signers are oracle identities exactly as the oracle set and the key
        // registry hold them (byte for byte), strictly ascending, so each
        // identity signs at most once.
        if !self.signatures.iter().all(|s| is_plain_id(&s.oracle))
            || !self
                .signatures
                .windows(2)
                .all(|pair| pair[0].oracle.as_bytes() < pair[1].oracle.as_bytes())
        {
            return Err(Malformed(
                "signers must be oracle identities, strictly ascending",
            ));
        }
        if self.forecast_commit_ids.is_empty()
            || self.forecast_commit_ids.len() > MAX_FORECASTS_PER_SCORE_TX
        {
            return Err(Malformed("forecast_commit_ids count"));
        }
        if !self.forecast_commit_ids.iter().all(|id| is_plain_id(id))
            || !self
                .forecast_commit_ids
                .windows(2)
                .all(|pair| pair[0] < pair[1])
        {
            return Err(Malformed(
                "forecast_commit_ids must be plain ids, strictly ascending",
            ));
        }
        Ok(())
    }

    /// Every signer is an explicitly registered oracle identity, and every
    /// signature verifies against a key the registry holds active for that
    /// identity at `signed_at` (so a rotated-in key signs and a revoked key
    /// does not). Membership is checked for ALL signers before the first
    /// signature is verified. Mempool admission runs this; consensus runs
    /// [`Self::verify_quorum`].
    pub fn verify_registered_signers(
        &self,
        chain_id: &str,
        network_id: &str,
        registry: &SigningKeyRegistry,
        signed_at: i64,
    ) -> Result<(), ForecastLateScoreError> {
        self.validate()?;
        if !self
            .signatures
            .iter()
            .all(|s| registry.has_identity_role(&s.oracle, SigningRole::Oracle))
        {
            return Err(ForecastLateScoreError::UnauthorizedOracle);
        }
        let message = forecast_score_message(
            chain_id,
            network_id,
            &self.attestation,
            &self.forecast_commit_ids,
        );
        for signer in &self.signatures {
            if !registry.verify_signature_for_identity_role(
                &signer.oracle,
                SigningRole::Oracle,
                signed_at,
                &message,
                signer.ed25519.trim(),
            ) {
                return Err(ForecastLateScoreError::BadSignature);
            }
        }
        Ok(())
    }

    /// The consensus check. In this order: shape; EVERY signer is in the
    /// chain's oracle set and a registered oracle identity (a payload padded
    /// with an unauthorized signer is refused, not counted around); there are
    /// at least `quorum` of them (distinct by the ascending-order rule; a
    /// quorum above [`MAX_OBSERVATION_SIGNATURES`] can never be met and is
    /// refused here); and only then every signature verifies through the
    /// registry at `signed_at`, the block's time.
    pub fn verify_quorum(
        &self,
        chain_id: &str,
        network_id: &str,
        registry: &SigningKeyRegistry,
        in_oracle_set: impl Fn(&str) -> bool,
        quorum: usize,
        signed_at: i64,
    ) -> Result<(), ForecastLateScoreError> {
        self.validate()?;
        if !self.signatures.iter().all(|s| in_oracle_set(&s.oracle)) {
            return Err(ForecastLateScoreError::UnauthorizedOracle);
        }
        let need = quorum.max(1);
        if self.signatures.len() < need {
            return Err(ForecastLateScoreError::BelowQuorum {
                have: self.signatures.len(),
                need,
            });
        }
        self.verify_registered_signers(chain_id, network_id, registry, signed_at)
    }
}

/// A forecast commit waiting for its reveal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingForecast {
    /// The address of the key that signed the commit.
    pub forecaster: Address,
    pub commit_height: u64,
    /// Time of the commit block. Stamped by the bucket close that ends the
    /// commit block (the commit transition carries no time); a reveal in the
    /// commit block itself takes its own block's time.
    pub commit_time: Option<i64>,
    /// Last height the reveal may land; the entry is dropped after it.
    pub expires_at_height: u64,
}

/// One recorded forecast, by commit id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForecastRecord {
    pub forecaster: Address,
    pub station_id: String,
    pub variable: LateScoreVariable,
    pub value: i64,
    pub lead_hours: u32,
    pub commit_height: u64,
    /// Time of the commit block.
    pub commit_time: i64,
    /// `commit_time + lead`: the time scored.
    pub target: i64,
    pub reveal_height: u64,
    /// Skill in millionths once scored.
    pub skill: Option<u64>,
}

/// The forecasts competing for one (station, variable, lead, target hour).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForecastBucket {
    pub station_id: String,
    pub variable: LateScoreVariable,
    pub lead_hours: u32,
    /// Start of the hour the targets fall in.
    pub hour: i64,
    /// The observation the forecasts of a commit block were first scored
    /// against: commit height → [`observation_digest`].
    pub observations: BTreeMap<u64, String>,
    /// Commit ids in reveal order, at most [`MAX_BUCKET_ENTRIES`], one per
    /// forecaster.
    pub commit_ids: Vec<String>,
}

/// The forecast pool ledger of one epoch.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForecastPoolEpoch {
    /// The most this epoch may pay.
    pub budget: Balance,
    /// Paid so far.
    pub paid: Balance,
    /// Earned by closed buckets but not paid (a cap or the budget was hit).
    /// Never minted.
    pub unissued: Balance,
    /// Paid so far per forecaster, for the per-forecaster cap. Keyed by
    /// [`forecaster_key`] (a string, so the state also serializes as JSON).
    pub paid_by_forecaster: BTreeMap<String, Balance>,
}

/// All state of the lane. Empty until the feature is in force.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForecastLateScoreState {
    /// Forecast commits waiting for their reveal, by commit id.
    #[serde(default)]
    pub pending: BTreeMap<String, PendingForecast>,
    /// Recorded forecasts, by commit id.
    #[serde(default)]
    pub records: BTreeMap<String, ForecastRecord>,
    /// Open buckets, by [`bucket_id`].
    #[serde(default)]
    pub buckets: BTreeMap<String, ForecastBucket>,
    /// Pool ledger, by epoch (`height / POOL_EPOCH_BLOCKS`).
    #[serde(default)]
    pub pool: BTreeMap<u64, ForecastPoolEpoch>,
}

/// The key of a forecaster in [`ForecastPoolEpoch::paid_by_forecaster`]: the
/// 40 lowercase hex characters of the address.
pub fn forecaster_key(forecaster: &Address) -> String {
    hex::encode(forecaster.0)
}

/// One amount a closed bucket earned, before the epoch and block limits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BucketAward {
    pub bucket_id: String,
    pub commit_id: String,
    pub forecaster: Address,
    pub skill: u64,
    /// After the per-bucket cap.
    pub amount: u128,
}

/// One payment decided by [`ForecastLateScoreState::settle_awards`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForecastPayment {
    pub forecaster: Address,
    pub commit_id: String,
    pub amount: u128,
}

/// Split `budget` over `nominal` pro rata, in order, with floored division;
/// never more than a nominal amount and never more than `budget` in total.
/// Fail closed: `None` on overflow.
pub fn allocate_pro_rata(nominal: &[u128], budget: u128) -> Option<Vec<u128>> {
    let mut remaining_nominal = nominal
        .iter()
        .try_fold(0u128, |sum, amount| sum.checked_add(*amount))?;
    if remaining_nominal <= budget {
        return Some(nominal.to_vec());
    }
    let mut remaining_budget = budget;
    let mut out = Vec::with_capacity(nominal.len());
    for amount in nominal {
        let share = if remaining_nominal == 0 {
            0
        } else {
            amount
                .checked_mul(remaining_budget)?
                .checked_div(remaining_nominal)?
                .min(*amount)
        };
        remaining_nominal = remaining_nominal.checked_sub(*amount)?;
        remaining_budget = remaining_budget.checked_sub(share)?;
        out.push(share);
    }
    Some(out)
}

/// The integer value a forecast reveal carries for this lane: exactly one
/// supported key holding a JSON integer inside the variable's range (rule 1).
/// Anything else (two supported keys, a float, a string, out of range) is not
/// recorded. No float is ever parsed or rounded here.
pub fn forecast_value(data: &serde_json::Value) -> Option<(LateScoreVariable, i64)> {
    let object = data.as_object()?;
    let mut found = None;
    for variable in LateScoreVariable::ALL {
        if let Some(value) = object.get(data_key(variable)) {
            if found.is_some() {
                return None;
            }
            // `as_i64` is `None` for floats, strings and integers outside i64.
            let value = value.as_i64().filter(|v| variable.in_range(*v))?;
            found = Some((variable, value));
        }
    }
    found
}

/// What the chain knows of a reading cited by an attestation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CitedReading {
    pub station_id: String,
    /// Height of the block the reading's reveal landed in.
    pub landed_at_height: u64,
    /// Whether it has reached the oracle quorum now.
    pub has_quorum: bool,
    /// Whether its recorded provenance class is `synthetic`.
    pub synthetic: bool,
    /// Whether the reveal carried a weather observation (a station reading),
    /// as opposed to a forecast or any other reveal.
    pub is_weather_reading: bool,
}

/// The readings an attestation cites must be on-chain readings of the
/// attested station at oracle quorum, and every window reading must have
/// landed below the forecasts' commit height. A reading whose provenance
/// class is `synthetic` is refused as truth and as a window reading (no class
/// recorded, or any other class, counts). `reading` resolves a commit id when
/// it is an applied contribution reveal.
pub fn check_cited_readings(
    attestation: &ObservationAttestation,
    reading: impl Fn(&str) -> Option<CitedReading>,
) -> Result<(), ForecastLateScoreError> {
    let counts = |commit_id: &str| {
        reading(commit_id).filter(|r| {
            r.has_quorum && r.is_weather_reading && r.station_id == attestation.station_id
        })
    };
    let truth =
        counts(&attestation.truth_commit_id).ok_or(ForecastLateScoreError::TruthNotAttested)?;
    if truth.synthetic {
        return Err(ForecastLateScoreError::SyntheticReading);
    }
    // The truth is observed after the forecast: it landed above the commit
    // height, and so can never also be a window reading.
    if truth.landed_at_height <= attestation.commit_height
        || attestation
            .window_commit_ids
            .contains(&attestation.truth_commit_id)
    {
        return Err(ForecastLateScoreError::TruthNotAfterCommit);
    }
    for commit_id in &attestation.window_commit_ids {
        let cited = counts(commit_id).ok_or(ForecastLateScoreError::ReadingNotAttested)?;
        if cited.synthetic {
            return Err(ForecastLateScoreError::SyntheticReading);
        }
        if cited.landed_at_height >= attestation.commit_height {
            return Err(ForecastLateScoreError::ReadingAfterCommit);
        }
    }
    Ok(())
}

impl ForecastLateScoreState {
    /// Read-only admission pre-check for a node that holds this state (an API
    /// route, a gossip ingest). NOT a consensus rule: apply
    /// ([`Self::apply_score`]) skips unknown and already-scored forecasts so
    /// that racing relays do not fail each other. Here every listed forecast
    /// must be recorded, unscored, and of the attested station, variable,
    /// lead, commit block and target.
    pub fn check_admissible_score(
        &self,
        payload: &ForecastScorePayload,
    ) -> Result<(), ForecastLateScoreError> {
        let a = &payload.attestation;
        for commit_id in &payload.forecast_commit_ids {
            let record = self
                .records
                .get(commit_id)
                .ok_or_else(|| ForecastLateScoreError::ForecastNotInBucket(commit_id.clone()))?;
            if record.station_id != a.station_id
                || record.variable != a.variable
                || record.lead_hours != a.lead_hours
                || record.commit_height != a.commit_height
                || record.commit_time != a.commit_time
                || record.target != a.target
            {
                return Err(ForecastLateScoreError::ForecastNotInBucket(commit_id.clone()));
            }
            if record.skill.is_some() {
                return Err(ForecastLateScoreError::AlreadyScored(commit_id.clone()));
            }
        }
        Ok(())
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
            && self.records.is_empty()
            && self.buckets.is_empty()
            && self.pool.is_empty()
    }

    /// A forecast commit was registered: remember who signed it.
    pub fn note_commit(
        &mut self,
        commit_id: &str,
        forecaster: Address,
        commit_height: u64,
        expires_at_height: u64,
    ) {
        self.pending.insert(
            commit_id.to_string(),
            PendingForecast {
                forecaster,
                commit_height,
                commit_time: None,
                expires_at_height,
            },
        );
    }

    /// A forecast reveal applied at `block_height` / `block_timestamp`. Stores
    /// a record when the forecast qualifies; otherwise does nothing. Never an
    /// error: this must not change whether a reveal is valid.
    ///
    /// `station_owner` is the station's registered owner, `None` when the
    /// station is not registered (then nothing is recorded).
    pub fn note_reveal(
        &mut self,
        payload: &RevealPayload,
        block_height: u64,
        block_timestamp: i64,
        station_owner: Option<Address>,
    ) -> bool {
        let Some(pending) = self.pending.remove(&payload.commit_id) else {
            return false;
        };
        let Some(owner) = station_owner else {
            return false;
        };
        // A station owner forecasting their own station is not paid.
        if owner == pending.forecaster
            || !is_plain_id(&payload.station_id)
            || !is_plain_id(&payload.commit_id)
        {
            return false;
        }
        // The commit block's time: stamped when that block closed, or this
        // block's own time when commit and reveal share a block.
        let commit_time = match pending.commit_time {
            Some(time) => time,
            None if pending.commit_height == block_height => block_timestamp,
            None => return false,
        };
        let Some(lead_hours) = payload.manifest.as_ref().map(|m| m.horizon_hours) else {
            return false;
        };
        let Some((variable, value)) = forecast_value(&payload.data) else {
            return false;
        };
        let Some(target) = target_for(commit_time, lead_hours) else {
            return false;
        };
        // No bucket whose close time does not fit an i64 (absurd timestamps).
        let Some(hour) = bucket_hour(target)
            .filter(|hour| *hour <= i64::MAX - SECS_PER_HOUR - SCORE_WINDOW_SECS)
        else {
            return false;
        };
        let Some(id) = bucket_id(&payload.station_id, variable, lead_hours, hour) else {
            return false;
        };
        if self.records.contains_key(&payload.commit_id) {
            return false;
        }
        if let Some(bucket) = self.buckets.get(&id) {
            // Full, or this wallet already has its first forecast here.
            let already = bucket
                .commit_ids
                .iter()
                .filter_map(|commit_id| self.records.get(commit_id))
                .any(|record| record.forecaster == pending.forecaster);
            if already || bucket.commit_ids.len() >= MAX_BUCKET_ENTRIES {
                return false;
            }
        }
        self.buckets
            .entry(id)
            .or_insert_with(|| ForecastBucket {
                station_id: payload.station_id.clone(),
                variable,
                lead_hours,
                hour,
                observations: BTreeMap::new(),
                commit_ids: Vec::new(),
            })
            .commit_ids
            .push(payload.commit_id.clone());
        self.records.insert(
            payload.commit_id.clone(),
            ForecastRecord {
                forecaster: pending.forecaster,
                station_id: payload.station_id.clone(),
                variable,
                value,
                lead_hours,
                commit_height: pending.commit_height,
                commit_time,
                target,
                reveal_height: block_height,
                skill: None,
            },
        );
        true
    }

    /// Score the listed forecasts against the payload's observation at
    /// `block_timestamp`. The caller has already verified the oracle quorum
    /// and the cited readings. All checks run before the first write, so a
    /// refusal leaves the state untouched. Returns how many forecasts were
    /// newly scored (at least one).
    pub fn apply_score(
        &mut self,
        payload: &ForecastScorePayload,
        digest: &str,
        block_timestamp: i64,
    ) -> Result<usize, ForecastLateScoreError> {
        use ForecastLateScoreError::Overflow;
        let attestation = &payload.attestation;
        // Not before target + 35 min: the truth candidates are then closed.
        let opens_at = attestation
            .target
            .checked_add(SCORE_OPENS_AFTER_SECS)
            .ok_or(Overflow)?;
        if block_timestamp < opens_at {
            return Err(ForecastLateScoreError::TooEarly { opens_at });
        }
        let hour =
            bucket_hour(attestation.target).ok_or(ForecastLateScoreError::Malformed("target"))?;
        let closes_at = bucket_closes_at(hour);
        if block_timestamp >= closes_at {
            return Err(ForecastLateScoreError::Closed { closes_at });
        }
        let id = bucket_id(
            &attestation.station_id,
            attestation.variable,
            attestation.lead_hours,
            hour,
        )
        .ok_or(ForecastLateScoreError::Malformed("target"))?;
        let bucket = self
            .buckets
            .get(&id)
            .ok_or(ForecastLateScoreError::UnknownBucket)?;
        if bucket
            .observations
            .get(&attestation.commit_height)
            .is_some_and(|pinned| pinned != digest)
        {
            return Err(ForecastLateScoreError::ObservationConflict);
        }
        let mut scored = Vec::new();
        for commit_id in &payload.forecast_commit_ids {
            // Unknown (never recorded, or already closed) and already scored
            // forecasts are skipped, so two relays racing do not fail each other.
            let Some(record) = self.records.get(commit_id) else {
                continue;
            };
            // The window and `last` belong to one commit block: a forecast
            // from any other block, bucket or time is a hard refusal.
            if !bucket.commit_ids.contains(commit_id)
                || record.commit_height != attestation.commit_height
                || record.commit_time != attestation.commit_time
                || record.target != attestation.target
            {
                return Err(ForecastLateScoreError::ForecastNotInBucket(
                    commit_id.clone(),
                ));
            }
            if record.skill.is_some() {
                continue;
            }
            let score = late_skill(&LateScoreInput {
                variable: attestation.variable,
                forecast: record.value,
                truth: attestation.truth,
                reference: attestation.reference,
                mean24: attestation.mean24,
                last: attestation.last,
                tolerance: copy_tolerance(attestation.variable),
            })
            .map_err(|e| ForecastLateScoreError::Score(e.to_string()))?;
            scored.push((commit_id.clone(), score.skill));
        }
        if scored.is_empty() {
            return Err(ForecastLateScoreError::NothingToScore);
        }
        let count = scored.len();
        for (commit_id, skill) in scored {
            if let Some(record) = self.records.get_mut(&commit_id) {
                record.skill = Some(skill);
            }
        }
        if let Some(bucket) = self.buckets.get_mut(&id) {
            bucket
                .observations
                .entry(attestation.commit_height)
                .or_insert_with(|| digest.to_string());
        }
        Ok(count)
    }

    /// End-of-block step, once per block while the feature is in force:
    /// stamp this block's time on the forecast commits it carried, close every
    /// bucket due at `block_timestamp` (oldest first, at most
    /// [`MAX_BUCKET_CLOSES_PER_BLOCK`]), drop expired pending commits and old
    /// pool epochs, and return what the closed buckets earned, in bucket order
    /// then rank. Closed buckets and their records are deleted whether or not
    /// they earned anything.
    ///
    /// `station_owner` resolves a station's registered owner now: a forecaster
    /// who owns the station at close is not paid either.
    pub fn close_due(
        &mut self,
        block_height: u64,
        block_timestamp: i64,
        station_owner: impl Fn(&str) -> Option<Address>,
    ) -> Vec<BucketAward> {
        for pending in self.pending.values_mut() {
            if pending.commit_height == block_height && pending.commit_time.is_none() {
                pending.commit_time = Some(block_timestamp);
            }
        }
        self.pending
            .retain(|_, pending| pending.expires_at_height >= block_height);
        self.pool
            .retain(|epoch, _| !pool_epoch_expired(*epoch, block_height));

        // Ids sort by hour, so the due buckets are a prefix.
        let due: Vec<String> = self
            .buckets
            .iter()
            .take_while(|(_, bucket)| bucket_is_due(bucket, block_timestamp))
            .take(MAX_BUCKET_CLOSES_PER_BLOCK)
            .map(|(id, _)| id.clone())
            .collect();
        let mut awards = Vec::new();
        for id in due {
            let Some(bucket) = self.buckets.remove(&id) else {
                continue;
            };
            let entries: Vec<(String, ForecastRecord)> = bucket
                .commit_ids
                .iter()
                .filter_map(|commit_id| {
                    self.records
                        .remove(commit_id)
                        .map(|record| (commit_id.clone(), record))
                })
                .collect();
            let owner = station_owner(&bucket.station_id);
            // Arithmetic that cannot be completed pays nothing for the bucket;
            // it never stops the block (fail closed).
            awards.extend(bucket_awards(&id, &bucket, &entries, owner).unwrap_or_default());
        }
        awards
    }

    /// Fit `awards` into the forecaster cap, the epoch budget and
    /// `block_budget`, record the result in the epoch ledger, and return the
    /// payments to mint (zero amounts omitted). `epoch_budget` is used only
    /// when this is the epoch's first settlement.
    ///
    /// Order of limits: each award is first cut to what its forecaster may
    /// still earn this epoch; then all awards are scaled pro rata to
    /// `min(epoch budget left, block_budget)`. Whatever is cut is recorded as
    /// unissued and never minted.
    pub fn settle_awards(
        &mut self,
        block_height: u64,
        awards: &[BucketAward],
        epoch_budget: u128,
        block_budget: u128,
    ) -> Result<Vec<ForecastPayment>, ForecastLateScoreError> {
        use ForecastLateScoreError::Overflow;
        if awards.is_empty() {
            return Ok(Vec::new());
        }
        let epoch = block_height / POOL_EPOCH_BLOCKS;
        let mut ledger = self
            .pool
            .get(&epoch)
            .cloned()
            .unwrap_or_else(|| ForecastPoolEpoch {
                budget: Balance::new(epoch_budget),
                ..ForecastPoolEpoch::default()
            });
        // The budget is fixed by the first settlement of the epoch that sees
        // a non-zero slice; a zero first slice does not zero the epoch.
        if ledger.budget.into_inner() == 0 {
            ledger.budget = Balance::new(epoch_budget);
        }
        let budget = ledger.budget.into_inner();
        let forecaster_cap = budget
            .checked_mul(FORECASTER_EPOCH_SHARE_BPS)
            .ok_or(Overflow)?
            / BPS;

        // 1. Per-forecaster cap, in award order.
        let mut running: BTreeMap<Address, u128> = BTreeMap::new();
        let mut capped = Vec::with_capacity(awards.len());
        for award in awards {
            let before = match running.get(&award.forecaster) {
                Some(amount) => *amount,
                None => ledger
                    .paid_by_forecaster
                    .get(&forecaster_key(&award.forecaster))
                    .map_or(0, |b| b.into_inner()),
            };
            let allowed = award.amount.min(forecaster_cap.saturating_sub(before));
            running.insert(
                award.forecaster,
                before.checked_add(allowed).ok_or(Overflow)?,
            );
            capped.push(allowed);
        }
        // 2. Epoch budget and block budget, pro rata.
        let available = budget
            .saturating_sub(ledger.paid.into_inner())
            .min(block_budget);
        let paid = allocate_pro_rata(&capped, available).ok_or(Overflow)?;

        let mut payments = Vec::new();
        let mut total_paid = 0u128;
        let mut total_earned = 0u128;
        for (award, amount) in awards.iter().zip(paid) {
            total_earned = total_earned.checked_add(award.amount).ok_or(Overflow)?;
            if amount == 0 {
                continue;
            }
            total_paid = total_paid.checked_add(amount).ok_or(Overflow)?;
            let entry = ledger
                .paid_by_forecaster
                .entry(forecaster_key(&award.forecaster))
                .or_default();
            *entry = Balance::new(entry.into_inner().checked_add(amount).ok_or(Overflow)?);
            payments.push(ForecastPayment {
                forecaster: award.forecaster,
                commit_id: award.commit_id.clone(),
                amount,
            });
        }
        ledger.paid = Balance::new(
            ledger
                .paid
                .into_inner()
                .checked_add(total_paid)
                .ok_or(Overflow)?,
        );
        let cut = total_earned.checked_sub(total_paid).ok_or(Overflow)?;
        ledger.unissued = Balance::new(
            ledger
                .unissued
                .into_inner()
                .checked_add(cut)
                .ok_or(Overflow)?,
        );
        self.pool.insert(epoch, ledger);
        Ok(payments)
    }

    /// Move `amount` of an epoch's paid total back to unissued: a payment that
    /// was planned but could not be minted.
    pub fn mark_unminted(&mut self, block_height: u64, forecaster: &Address, amount: u128) {
        let Some(ledger) = self.pool.get_mut(&(block_height / POOL_EPOCH_BLOCKS)) else {
            return;
        };
        ledger.paid = Balance::new(ledger.paid.into_inner().saturating_sub(amount));
        ledger.unissued = Balance::new(ledger.unissued.into_inner().saturating_add(amount));
        if let Some(paid) = ledger
            .paid_by_forecaster
            .get_mut(&forecaster_key(forecaster))
        {
            *paid = Balance::new(paid.into_inner().saturating_sub(amount));
        }
    }

    /// Append this state to the state-root pre-image. Called only when the
    /// state is not empty. Stable, length-prefixed encoding in key order.
    pub fn root_material(&self, material: &mut Vec<u8>) {
        fn u64le(out: &mut Vec<u8>, value: u64) {
            out.extend_from_slice(&value.to_le_bytes());
        }
        fn i64le(out: &mut Vec<u8>, value: i64) {
            out.extend_from_slice(&value.to_le_bytes());
        }
        fn u128le(out: &mut Vec<u8>, value: Balance) {
            out.extend_from_slice(&value.into_inner().to_le_bytes());
        }
        material.extend_from_slice(ROOT_MATERIAL_TAG);
        u64le(material, self.pending.len() as u64);
        for (commit_id, pending) in &self.pending {
            push(material, commit_id.as_bytes());
            material.extend_from_slice(&pending.forecaster.0);
            u64le(material, pending.commit_height);
            match pending.commit_time {
                Some(time) => {
                    material.push(1);
                    i64le(material, time);
                },
                None => material.push(0),
            }
            u64le(material, pending.expires_at_height);
        }
        u64le(material, self.records.len() as u64);
        for (commit_id, record) in &self.records {
            push(material, commit_id.as_bytes());
            material.extend_from_slice(&record.forecaster.0);
            push(material, record.station_id.as_bytes());
            material.push(wire_byte(record.variable));
            i64le(material, record.value);
            u64le(material, u64::from(record.lead_hours));
            u64le(material, record.commit_height);
            i64le(material, record.commit_time);
            i64le(material, record.target);
            u64le(material, record.reveal_height);
            match record.skill {
                Some(skill) => {
                    material.push(1);
                    u64le(material, skill);
                },
                None => material.push(0),
            }
        }
        u64le(material, self.buckets.len() as u64);
        for (id, bucket) in &self.buckets {
            push(material, id.as_bytes());
            push(material, bucket.station_id.as_bytes());
            material.push(wire_byte(bucket.variable));
            u64le(material, u64::from(bucket.lead_hours));
            i64le(material, bucket.hour);
            u64le(material, bucket.observations.len() as u64);
            for (commit_height, digest) in &bucket.observations {
                u64le(material, *commit_height);
                push(material, digest.as_bytes());
            }
            u64le(material, bucket.commit_ids.len() as u64);
            for commit_id in &bucket.commit_ids {
                push(material, commit_id.as_bytes());
            }
        }
        u64le(material, self.pool.len() as u64);
        for (epoch, ledger) in &self.pool {
            u64le(material, *epoch);
            u128le(material, ledger.budget);
            u128le(material, ledger.paid);
            u128le(material, ledger.unissued);
            u64le(material, ledger.paid_by_forecaster.len() as u64);
            for (forecaster, paid) in &ledger.paid_by_forecaster {
                push(material, forecaster.as_bytes());
                u128le(material, *paid);
            }
        }
    }
}

/// When the bucket of `hour` closes: 24 h after the end of the hour, so every
/// forecast in it has had at least 24 h after its target to be scored.
/// Saturating: [`ForecastLateScoreState::note_reveal`] never creates a bucket
/// whose close time would not fit.
pub const fn bucket_closes_at(hour: i64) -> i64 {
    hour.saturating_add(SECS_PER_HOUR + SCORE_WINDOW_SECS)
}

fn bucket_is_due(bucket: &ForecastBucket, block_timestamp: i64) -> bool {
    block_timestamp >= bucket_closes_at(bucket.hour)
}

fn pool_epoch_expired(epoch: u64, block_height: u64) -> bool {
    epoch.saturating_add(POOL_EPOCHS_RETAINED) < block_height / POOL_EPOCH_BLOCKS
}

/// The epoch budget for an epoch whose first settlement happens in a block
/// with Contribution slice `contribution_slice`:
/// `slice × POOL_EPOCH_BLOCKS × POOL_SHARE_BPS / 10_000`.
pub fn epoch_budget(contribution_slice: u128) -> Option<u128> {
    contribution_slice
        .checked_mul(u128::from(POOL_EPOCH_BLOCKS))?
        .checked_mul(POOL_SHARE_BPS)
        .map(|n| n / BPS)
}

/// The most the lane may take of a block whose Contribution slice is
/// `contribution_slice`.
pub fn block_share(contribution_slice: u128) -> Option<u128> {
    contribution_slice
        .checked_mul(BLOCK_SHARE_BPS)
        .map(|n| n / BPS)
}

/// What one closed bucket earns: its top [`PAID_PER_BUCKET`] forecasters by
/// skill (ties: earlier commit, then commit id), `skill × SKILL_UNIT / 10^6`
/// each, scaled pro rata to [`BUCKET_CAP`].
///
/// A forecast earns nothing when it is unscored, scored zero, a copy, or its
/// forecaster owns the station. A copy: another forecaster's forecast in the
/// bucket was revealed BEFORE this one was committed and is within the copy
/// tolerance of it. Forecasts committed in or before the block the other was
/// revealed in cannot have copied it.
fn bucket_awards(
    id: &str,
    bucket: &ForecastBucket,
    entries: &[(String, ForecastRecord)],
    station_owner: Option<Address>,
) -> Result<Vec<BucketAward>, ForecastLateScoreError> {
    let tolerance = copy_tolerance(bucket.variable);
    let is_copy = |record: &ForecastRecord| {
        entries.iter().any(|(_, other)| {
            other.forecaster != record.forecaster
                && other.reveal_height < record.commit_height
                && other.value.abs_diff(record.value) <= tolerance
        })
    };
    let mut ranked: Vec<(u64, u64, &String, Address)> = entries
        .iter()
        .filter_map(|(commit_id, record)| {
            let skill = record
                .skill
                .filter(|skill| *skill > 0 && *skill <= SKILL_PRECISION)?;
            if Some(record.forecaster) == station_owner || is_copy(record) {
                return None;
            }
            Some((skill, record.commit_height, commit_id, record.forecaster))
        })
        .collect();
    ranked.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)).then(a.2.cmp(b.2)));
    ranked.truncate(PAID_PER_BUCKET);
    let nominal = ranked
        .iter()
        .map(|(skill, ..)| {
            // skill ≤ 10^6 and SKILL_UNIT = 10^8: the product is ≤ 10^14.
            u128::from(*skill)
                .checked_mul(SKILL_UNIT)
                .map(|n| n / u128::from(SKILL_PRECISION))
        })
        .collect::<Option<Vec<u128>>>()
        .ok_or(ForecastLateScoreError::Overflow)?;
    let amounts =
        allocate_pro_rata(&nominal, BUCKET_CAP).ok_or(ForecastLateScoreError::Overflow)?;
    Ok(ranked
        .into_iter()
        .zip(amounts)
        .map(|((skill, _, commit_id, forecaster), amount)| BucketAward {
            bucket_id: id.to_string(),
            commit_id: commit_id.clone(),
            forecaster,
            skill,
            amount,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(byte: u8) -> Address {
        Address([byte; 20])
    }

    fn record(
        forecaster: u8,
        value: i64,
        commit: u64,
        reveal: u64,
        skill: Option<u64>,
    ) -> ForecastRecord {
        ForecastRecord {
            forecaster: addr(forecaster),
            station_id: "st-1".into(),
            variable: LateScoreVariable::TemperatureC,
            value,
            lead_hours: 6,
            commit_height: commit,
            commit_time: 1_000,
            target: 22_600,
            reveal_height: reveal,
            skill,
        }
    }

    fn bucket() -> ForecastBucket {
        ForecastBucket {
            station_id: "st-1".into(),
            variable: LateScoreVariable::TemperatureC,
            lead_hours: 6,
            hour: 21_600,
            observations: BTreeMap::new(),
            commit_ids: Vec::new(),
        }
    }

    fn awards(entries: &[(&str, ForecastRecord)], owner: Option<Address>) -> Vec<(String, u128)> {
        let entries: Vec<(String, ForecastRecord)> = entries
            .iter()
            .map(|(id, r)| ((*id).to_string(), r.clone()))
            .collect();
        bucket_awards("b", &bucket(), &entries, owner)
            .unwrap()
            .into_iter()
            .map(|a| (a.commit_id, a.amount))
            .collect()
    }

    #[test]
    fn pro_rata_never_exceeds_budget_or_nominal() {
        assert_eq!(allocate_pro_rata(&[10, 20, 30], 60), Some(vec![10, 20, 30]));
        assert_eq!(
            allocate_pro_rata(&[10, 20, 30], 1_000),
            Some(vec![10, 20, 30])
        );
        assert_eq!(allocate_pro_rata(&[10, 20, 30], 30), Some(vec![5, 10, 15]));
        assert_eq!(allocate_pro_rata(&[1, 1, 1], 2), Some(vec![0, 1, 1]));
        assert_eq!(allocate_pro_rata(&[7, 0, 5], 0), Some(vec![0, 0, 0]));
        assert_eq!(allocate_pro_rata(&[], 5), Some(vec![]));
        assert_eq!(allocate_pro_rata(&[u128::MAX, 1], 5), None);
        for budget in 0..=40u128 {
            let nominal = [13u128, 0, 9, 17, 1];
            let out = allocate_pro_rata(&nominal, budget).unwrap();
            assert!(out.iter().sum::<u128>() <= budget.max(0));
            assert!(
                out.iter()
                    .zip(nominal)
                    .all(|(paid, nominal)| *paid <= nominal)
            );
            if budget >= 40 {
                assert_eq!(out, nominal.to_vec());
            }
        }
    }

    #[test]
    fn bucket_pays_top_three_by_skill_then_earlier_commit() {
        let got = awards(
            &[
                ("a", record(1, 100, 10, 12, Some(400_000))),
                ("b", record(2, 200, 11, 12, Some(900_000))),
                ("c", record(3, 300, 9, 12, Some(400_000))),
                ("d", record(4, 400, 10, 12, Some(100_000))),
                ("e", record(5, 500, 10, 12, Some(0))),
                ("f", record(6, 600, 10, 12, None)),
            ],
            None,
        );
        // 0.9 + 0.4 + 0.4 WRF = 1.7 WRF, under the 2 WRF cap.
        assert_eq!(
            got,
            vec![
                ("b".to_string(), 90_000_000),
                ("c".to_string(), 40_000_000),
                ("a".to_string(), 40_000_000)
            ]
        );
    }

    #[test]
    fn bucket_cap_scales_pro_rata() {
        let got = awards(
            &[
                ("a", record(1, 100, 10, 12, Some(1_000_000))),
                ("b", record(2, 200, 10, 12, Some(1_000_000))),
                ("c", record(3, 300, 10, 12, Some(1_000_000))),
            ],
            None,
        );
        let total: u128 = got.iter().map(|(_, amount)| amount).sum();
        assert!(total <= BUCKET_CAP);
        assert_eq!(
            got.iter().map(|(_, a)| *a).collect::<Vec<_>>(),
            vec![66_666_666, 66_666_667, 66_666_667]
        );
    }

    #[test]
    fn copier_rule_needs_an_earlier_reveal_by_someone_else() {
        // "a" revealed at 12. "b" committed at 13 within tolerance: a copy.
        // "c" committed at 12 (same block as the reveal): not a copy.
        // "d" committed at 13 but 6 units away: not a copy.
        // "e" is "a"'s own wallet committing later: not a copy of itself.
        let got = awards(
            &[
                ("a", record(1, 2_000, 10, 12, Some(500_000))),
                ("b", record(2, 2_005, 13, 15, Some(900_000))),
                ("c", record(3, 1_995, 12, 14, Some(300_000))),
                ("d", record(4, 2_006, 13, 15, Some(200_000))),
            ],
            None,
        );
        assert_eq!(
            got.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>(),
            vec!["a", "c", "d"],
            "the copier is not paid and does not take a top-3 place"
        );
        let own = awards(
            &[
                ("a", record(1, 2_000, 10, 12, Some(500_000))),
                ("e", record(1, 2_001, 13, 15, Some(600_000))),
            ],
            None,
        );
        assert_eq!(own.len(), 2);
    }

    #[test]
    fn station_owner_is_not_paid() {
        let entries = [
            ("a", record(1, 100, 10, 12, Some(800_000))),
            ("b", record(2, 300, 10, 12, Some(500_000))),
        ];
        assert_eq!(
            awards(&entries, Some(addr(1))),
            vec![("b".to_string(), 50_000_000)]
        );
        assert_eq!(awards(&entries, Some(addr(9))).len(), 2);
    }

    fn award(forecaster: u8, commit: &str, amount: u128) -> BucketAward {
        BucketAward {
            bucket_id: "b".into(),
            commit_id: commit.into(),
            forecaster: addr(forecaster),
            skill: 1,
            amount,
        }
    }

    #[test]
    fn settle_applies_forecaster_cap_then_budgets_and_records_unissued() {
        let mut state = ForecastLateScoreState::default();
        // Epoch budget 1_000: forecaster cap 100.
        let payments = state
            .settle_awards(
                5_000,
                &[award(1, "a", 150), award(2, "b", 60), award(1, "c", 30)],
                1_000,
                u128::MAX,
            )
            .unwrap();
        assert_eq!(
            payments.iter().map(|p| p.amount).collect::<Vec<_>>(),
            vec![100, 60]
        );
        let ledger = &state.pool[&5];
        assert_eq!(ledger.budget, Balance::new(1_000));
        assert_eq!(ledger.paid, Balance::new(160));
        assert_eq!(ledger.unissued, Balance::new(80));
        assert_eq!(
            ledger.paid_by_forecaster[&forecaster_key(&addr(1))],
            Balance::new(100)
        );

        // Same epoch, later block: forecaster 1 is at the cap; the block
        // budget of 30 scales the rest. A different epoch_budget is ignored.
        let payments = state
            .settle_awards(
                5_999,
                &[award(1, "d", 50), award(3, "e", 40), award(4, "f", 20)],
                9,
                30,
            )
            .unwrap();
        assert_eq!(
            payments
                .iter()
                .map(|p| (p.commit_id.as_str(), p.amount))
                .collect::<Vec<_>>(),
            vec![("e", 20), ("f", 10)]
        );
        let ledger = &state.pool[&5];
        assert_eq!(ledger.budget, Balance::new(1_000));
        assert_eq!(ledger.paid, Balance::new(190));
        assert_eq!(ledger.unissued, Balance::new(160));

        // A zero first slice does not zero the epoch: the budget is taken from
        // the first settlement that sees a non-zero one, then stays fixed.
        let mut state = ForecastLateScoreState::default();
        assert!(
            state
                .settle_awards(7, &[award(1, "a", 50)], 0, u128::MAX)
                .unwrap()
                .is_empty()
        );
        assert_eq!(state.pool[&0].budget, Balance::new(0));
        let payments = state
            .settle_awards(8, &[award(2, "b", 50)], 1_000, u128::MAX)
            .unwrap();
        assert_eq!(payments.iter().map(|p| p.amount).sum::<u128>(), 50);
        assert_eq!(state.pool[&0].budget, Balance::new(1_000));
        state
            .settle_awards(9, &[award(3, "c", 50)], 9_999, u128::MAX)
            .unwrap();
        assert_eq!(state.pool[&0].budget, Balance::new(1_000));

        // The epoch budget is the outer limit.
        let mut state = ForecastLateScoreState::default();
        let many: Vec<BucketAward> = (0..20u8).map(|i| award(i, "x", 100)).collect();
        let payments = state.settle_awards(0, &many, 1_000, u128::MAX).unwrap();
        assert_eq!(payments.iter().map(|p| p.amount).sum::<u128>(), 1_000);
        assert_eq!(state.pool[&0].paid, Balance::new(1_000));
        assert_eq!(state.pool[&0].unissued, Balance::new(1_000));
        let later = state
            .settle_awards(1, &[award(30, "y", 10)], 1_000, u128::MAX)
            .unwrap();
        assert!(later.is_empty(), "an exhausted epoch pays nothing");
    }

    #[test]
    fn mark_unminted_moves_paid_back() {
        let mut state = ForecastLateScoreState::default();
        state
            .settle_awards(10, &[award(1, "a", 50)], 1_000, u128::MAX)
            .unwrap();
        state.mark_unminted(10, &addr(1), 50);
        let ledger = &state.pool[&0];
        assert_eq!(
            (ledger.paid, ledger.unissued),
            (Balance::new(0), Balance::new(50))
        );
        assert_eq!(
            ledger.paid_by_forecaster[&forecaster_key(&addr(1))],
            Balance::new(0)
        );
    }

    #[test]
    fn budgets_are_shares_of_the_contribution_slice() {
        assert_eq!(epoch_budget(1_000), Some(50_000));
        assert_eq!(block_share(1_000), Some(500));
        assert_eq!(epoch_budget(0), Some(0));
        assert_eq!(epoch_budget(u128::MAX), None);
        assert_eq!(block_share(u128::MAX), None);
    }

    #[test]
    fn target_is_commit_time_plus_lead_and_buckets_are_whole_hours() {
        assert_eq!(target_for(0, 1), Some(3_600));
        assert_eq!(target_for(1_762_646_461, 24), Some(1_762_732_861));
        for lead in [0, 2, 3, 5, 7, 23, 25, 48, u32::MAX] {
            assert_eq!(target_for(1_000, lead), None, "lead {lead}");
        }
        assert_eq!(target_for(-1, 1), None);
        assert_eq!(target_for(i64::MAX, 24), None);
        assert_eq!(bucket_hour(0), Some(0));
        assert_eq!(bucket_hour(3_599), Some(0));
        assert_eq!(bucket_hour(3_600), Some(3_600));
        assert_eq!(bucket_hour(1_762_732_861), Some(1_762_732_800));
        assert_eq!(bucket_hour(-1), None);
        // Every forecast of an hour has at least 24 h after its target.
        assert_eq!(bucket_closes_at(3_600), 3_600 + 3_600 + 86_400);
        assert_eq!(bucket_closes_at(i64::MAX - 10), i64::MAX);
        assert_eq!(SCORE_OPENS_AFTER_SECS, 35 * 60);
    }

    #[test]
    fn bucket_ids_sort_by_hour_and_separate_every_field() {
        let id =
            |station: &str, variable, lead, hour| bucket_id(station, variable, lead, hour).unwrap();
        let t = LateScoreVariable::TemperatureC;
        let base = id("st-1", t, 6, 7_200);
        assert!(
            base < id("st-0", t, 1, 10_800),
            "an earlier hour sorts first whatever the rest"
        );
        assert_ne!(base, id("st-2", t, 6, 7_200));
        assert_ne!(base, id("st-1", LateScoreVariable::PressureHpa, 6, 7_200));
        assert_ne!(base, id("st-1", t, 12, 7_200));
        assert_eq!(base, id("st-1", t, 6, 7_200));
        assert_eq!(bucket_id("st-1", t, 6, -3_600), None);
    }

    #[test]
    fn forecast_value_takes_exactly_one_in_range_integer() {
        use serde_json::json;
        let t = LateScoreVariable::TemperatureC;
        let p = LateScoreVariable::PressureHpa;
        assert_eq!(
            forecast_value(&json!({"temp_c_centi": 2137})),
            Some((t, 2137))
        );
        assert_eq!(
            forecast_value(&json!({"temp_c_centi": -40, "note": "x"})),
            Some((t, -40))
        );
        assert_eq!(
            forecast_value(&json!({"temp_c_centi": -10_000})),
            Some((t, -10_000))
        );
        assert_eq!(
            forecast_value(&json!({"temp_c_centi": 10_000})),
            Some((t, 10_000))
        );
        assert_eq!(
            forecast_value(&json!({"pressure_pa": 101_325})),
            Some((p, 101_325))
        );
        assert_eq!(
            forecast_value(&json!({"pressure_pa": 30_000})),
            Some((p, 30_000))
        );
        assert_eq!(
            forecast_value(&json!({"pressure_pa": 120_000})),
            Some((p, 120_000))
        );
        for bad in [
            json!({"temp_c_centi": 21.37}),
            json!({"temp_c_centi": 2137.0}),
            json!({"temp_c_centi": "2137"}),
            json!({"temp_c_centi": null}),
            json!({"temp_c_centi": 10_001}),
            json!({"temp_c_centi": -10_001}),
            json!({"temp_c_centi": u64::MAX}),
            json!({"pressure_pa": 29_999}),
            json!({"pressure_pa": 120_001}),
            json!({"pressure_pa": 1013.25}),
            json!({"temp_c_centi": 2137, "pressure_pa": 101_325}),
            json!({"temperature_c": 21}),
            json!([2137]),
            json!(2137),
        ] {
            assert_eq!(forecast_value(&bad), None, "{bad}");
        }
    }

    fn attestation() -> ObservationAttestation {
        ObservationAttestation {
            station_id: "st-1".into(),
            variable: LateScoreVariable::TemperatureC,
            lead_hours: 6,
            commit_height: 100,
            commit_time: 1_000,
            target: 22_600,
            truth: 2_000,
            truth_commit_id: "truth".into(),
            reference: 300,
            mean24: 1_900,
            last: 2_100,
            window_hash: "5a".repeat(32),
            window_commit_ids: vec!["w1".into(), "w2".into()],
        }
    }

    #[test]
    fn attestation_shape_and_range_rules() {
        assert_eq!(attestation().validate(), Ok(()));
        let bad = |change: fn(&mut ObservationAttestation)| {
            let mut a = attestation();
            change(&mut a);
            a.validate().is_err()
        };
        assert!(bad(|a| a.target += 1), "target must be commit_time + lead");
        assert!(bad(|a| a.lead_hours = 5));
        assert!(bad(|a| a.commit_time = -22_000));
        assert!(bad(|a| a.truth = 10_001));
        assert!(bad(|a| a.mean24 = -10_001));
        assert!(bad(|a| a.last = 30_000), "a pressure is not a temperature");
        assert!(bad(|a| a.reference = MAX_REFERENCE + 1));
        assert!(bad(|a| a.window_hash = "5A".repeat(32)));
        assert!(bad(|a| a.truth_commit_id = String::new()));
        assert!(bad(|a| a.window_commit_ids.clear()));
        assert!(bad(|a| a.window_commit_ids = vec!["w2".into(), "w1".into()]));
        assert!(bad(|a| a.window_commit_ids = vec!["w1".into(), "w1".into()]));
        assert!(bad(|a| a.window_commit_ids = (0..=MAX_WINDOW_READINGS)
            .map(|i| format!("w{i:05}"))
            .collect()));
        assert!(bad(|a| a.station_id = "has space".into()));
    }

    /// Signers are checked in a fixed order: shape, membership of every
    /// signer, the count, and only then signatures, through the registry.
    #[test]
    fn quorum_is_membership_then_count_then_registry_signatures() {
        use crate::crypto::signature::{PrivateKey, PublicKey};
        let keys: Vec<PrivateKey> = (1..=3u8)
            .map(|s| PrivateKey::from_bytes(&[s; 32]).unwrap())
            .collect();
        let id = |k: &PrivateKey| PublicKey::from_private_key(k).to_hex();
        let a = attestation();
        let message = forecast_score_message("888", "testnet", &a, &["f1".to_string()]);
        let mut signatures: Vec<ObservationSignature> = keys[..2]
            .iter()
            .map(|k| ObservationSignature {
                oracle: id(k),
                ed25519: k.sign(&message).to_hex(),
            })
            .collect();
        signatures.sort_by(|x, y| x.oracle.cmp(&y.oracle));
        let payload = ForecastScorePayload {
            attestation: a,
            signatures,
            forecast_commit_ids: vec!["f1".into()],
        };
        let mut registry = SigningKeyRegistry::default();
        for k in &keys[..2] {
            registry
                .ensure_legacy_identity_key(&id(k), SigningRole::Oracle, 0)
                .unwrap();
        }
        let check =
            |p: &ForecastScorePayload,
             r: &SigningKeyRegistry,
             member: &dyn Fn(&str) -> bool,
             quorum| { p.verify_quorum("888", "testnet", r, member, quorum, 10) };
        assert_eq!(check(&payload, &registry, &|_| true, 2), Ok(()));
        // Quorum 0 is treated as 1; a quorum nobody can reach is a clean refusal.
        assert_eq!(check(&payload, &registry, &|_| true, 0), Ok(()));
        for need in [3, MAX_OBSERVATION_SIGNATURES + 1, usize::MAX] {
            assert_eq!(
                check(&payload, &registry, &|_| true, need),
                Err(ForecastLateScoreError::BelowQuorum { have: 2, need })
            );
        }
        // One signer outside the oracle set refuses the payload before the
        // count, and before any signature is looked at.
        let outsider = payload.signatures[1].oracle.clone();
        let mut garbage = payload.clone();
        garbage.signatures[0].ed25519 = "00".repeat(64);
        assert_eq!(
            check(&garbage, &registry, &|o| o != outsider, 5),
            Err(ForecastLateScoreError::UnauthorizedOracle)
        );
        // In the set but with no registered key lifecycle: not an oracle. The
        // identity is never its own key here.
        let empty = SigningKeyRegistry::default();
        assert_eq!(
            check(&payload, &empty, &|_| true, 2),
            Err(ForecastLateScoreError::UnauthorizedOracle)
        );
        assert_eq!(
            payload.verify_registered_signers("888", "testnet", &empty, 10),
            Err(ForecastLateScoreError::UnauthorizedOracle)
        );
        assert_eq!(
            check(&garbage, &registry, &|_| true, 2),
            Err(ForecastLateScoreError::BadSignature)
        );
        // Another chain's signature does not verify here.
        assert_eq!(
            payload.verify_quorum("889", "testnet", &registry, |_| true, 2, 10),
            Err(ForecastLateScoreError::BadSignature)
        );
        // The identity is matched byte for byte: another spelling of the same
        // key is a different, unknown identity.
        let mut upper = payload.clone();
        upper.signatures.truncate(1);
        upper.signatures[0].oracle = upper.signatures[0].oracle.to_uppercase();
        assert_eq!(
            check(&upper, &registry, &|_| true, 1),
            Err(ForecastLateScoreError::UnauthorizedOracle)
        );
    }

    #[test]
    fn cited_readings_must_be_attested_station_readings_landed_before_the_commit() {
        let a = attestation();
        let chain = |id: &str| -> Option<CitedReading> {
            let (station, height, quorum, synthetic, weather) = match id {
                "truth" => ("st-1", 400, true, false, true),
                "w1" => ("st-1", 98, true, false, true),
                "w2" => ("st-1", 99, true, false, true),
                "at-commit" => ("st-1", 100, true, false, true),
                "after-commit" => ("st-1", 101, true, false, true),
                "no-quorum" => ("st-1", 50, false, false, true),
                "other-station" => ("st-2", 50, true, false, true),
                "synthetic" => ("st-1", 50, true, true, true),
                "forecast-reveal" => ("st-1", 50, true, false, false),
                _ => return None,
            };
            Some(CitedReading {
                station_id: station.into(),
                landed_at_height: height,
                has_quorum: quorum,
                synthetic,
                is_weather_reading: weather,
            })
        };
        assert_eq!(check_cited_readings(&a, chain), Ok(()));
        let with_window = |id: &str| {
            let mut a = attestation();
            a.window_commit_ids = vec![id.to_string()];
            check_cited_readings(&a, chain)
        };
        // A reading that landed in or after the commit block can never enter
        // the window, whatever observation time it claims.
        assert_eq!(
            with_window("at-commit"),
            Err(ForecastLateScoreError::ReadingAfterCommit)
        );
        assert_eq!(
            with_window("after-commit"),
            Err(ForecastLateScoreError::ReadingAfterCommit)
        );
        assert_eq!(
            with_window("no-quorum"),
            Err(ForecastLateScoreError::ReadingNotAttested)
        );
        assert_eq!(
            with_window("other-station"),
            Err(ForecastLateScoreError::ReadingNotAttested)
        );
        assert_eq!(
            with_window("unknown"),
            Err(ForecastLateScoreError::ReadingNotAttested)
        );
        // A reveal that is not a station reading (a forecast reveal, say) is
        // not a reading, as window or as truth.
        assert_eq!(
            with_window("forecast-reveal"),
            Err(ForecastLateScoreError::ReadingNotAttested)
        );
        // The truth must have landed after the commit, and cannot also be a
        // window reading.
        for early_truth in ["w1", "at-commit"] {
            let mut a = attestation();
            a.truth_commit_id = early_truth.into();
            a.window_commit_ids = vec!["w2".into()];
            assert_eq!(
                check_cited_readings(&a, chain),
                Err(ForecastLateScoreError::TruthNotAfterCommit)
            );
        }
        let mut both = attestation();
        both.window_commit_ids = vec!["truth".into()];
        assert_eq!(
            check_cited_readings(&both, chain),
            Err(ForecastLateScoreError::TruthNotAfterCommit)
        );
        // A synthetic reading is refused as a window reading and as the truth.
        assert_eq!(
            with_window("synthetic"),
            Err(ForecastLateScoreError::SyntheticReading)
        );
        let mut synthetic_truth = attestation();
        synthetic_truth.truth_commit_id = "synthetic".into();
        assert_eq!(
            check_cited_readings(&synthetic_truth, chain),
            Err(ForecastLateScoreError::SyntheticReading)
        );
        for truth in ["no-quorum", "other-station", "unknown", "forecast-reveal"] {
            let mut a = attestation();
            a.truth_commit_id = truth.into();
            assert_eq!(
                check_cited_readings(&a, chain),
                Err(ForecastLateScoreError::TruthNotAttested)
            );
        }
    }

    #[test]
    fn close_stamps_the_commit_time_of_this_block_only() {
        let mut state = ForecastLateScoreState::default();
        state.note_commit("c1", addr(1), 5, 8);
        state.note_commit("c2", addr(2), 6, 9);
        assert!(state.close_due(5, 111, |_| None).is_empty());
        assert_eq!(state.pending["c1"].commit_time, Some(111));
        assert_eq!(state.pending["c2"].commit_time, None);
        state.close_due(6, 222, |_| None);
        assert_eq!(
            state.pending["c1"].commit_time,
            Some(111),
            "a stamp is never rewritten"
        );
        assert_eq!(state.pending["c2"].commit_time, Some(222));
        state.close_due(9, 333, |_| None);
        assert!(
            !state.pending.contains_key("c1"),
            "expired after its reveal window"
        );
        assert!(state.pending.contains_key("c2"));
    }

    #[test]
    fn state_round_trips_through_json_and_bincode() {
        let mut state = ForecastLateScoreState::default();
        assert_eq!(bincode::serialize(&state).unwrap(), vec![0u8; 32]);
        state.note_commit("c0", addr(1), 5, 8);
        state.close_due(5, 99, |_| None);
        state
            .records
            .insert("c1".into(), record(1, -2_000, 5, 6, Some(7)));
        let mut scored = bucket();
        scored.observations.insert(5, "d".into());
        state.buckets.insert("b1".into(), scored);
        state
            .settle_awards(10, &[award(1, "a", 50)], 1_000, u128::MAX)
            .unwrap();
        let json = serde_json::to_string(&state).expect("json");
        assert_eq!(
            serde_json::from_str::<ForecastLateScoreState>(&json).expect("from json"),
            state
        );
        let bytes = bincode::serialize(&state).expect("bincode");
        assert_eq!(
            bincode::deserialize::<ForecastLateScoreState>(&bytes).expect("from bincode"),
            state
        );
        // An object with no members is the empty state (snapshot import).
        assert!(
            serde_json::from_str::<ForecastLateScoreState>("{}")
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn root_material_changes_with_every_part() {
        let root = |state: &ForecastLateScoreState| {
            let mut material = Vec::new();
            state.root_material(&mut material);
            material
        };
        let mut state = ForecastLateScoreState::default();
        let mut seen = vec![root(&state)];
        state.note_commit("c1", addr(1), 5, 8);
        seen.push(root(&state));
        state.close_due(5, 99, |_| None);
        seen.push(root(&state));
        state
            .records
            .insert("c1".into(), record(1, 2_000, 5, 6, None));
        seen.push(root(&state));
        state.records.get_mut("c1").unwrap().skill = Some(0);
        seen.push(root(&state));
        state.records.get_mut("c1").unwrap().target += 1;
        seen.push(root(&state));
        state.buckets.insert("b1".into(), bucket());
        seen.push(root(&state));
        state
            .buckets
            .get_mut("b1")
            .unwrap()
            .observations
            .insert(5, "d".into());
        seen.push(root(&state));
        state.pool.insert(
            3,
            ForecastPoolEpoch {
                budget: Balance::new(9),
                ..Default::default()
            },
        );
        seen.push(root(&state));
        let distinct: std::collections::BTreeSet<&Vec<u8>> = seen.iter().collect();
        assert_eq!(distinct.len(), seen.len());
    }
}
