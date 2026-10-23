use crate::blockchain::transaction::{
    CommitPayload, ForecastManifestValidationError, ForecastReceiptValidationError,
    RevealCidValidationError, RevealPayload, RevealProvenanceValidationError,
    canonicalize_calibration_attestation_cid, forecast_manifest_digest,
};
use crate::consensus::proof_of_publishing::{commit_window_blocks, reveal_window_blocks};
use crate::crypto::hash::hash_bytes;
use crate::station_registry::ContributionProvenanceClass;
use crate::weather::schema::{WeatherPayloadParseOptions, parse_weather_payload_with_options};
use crate::weather::validation::{
    BlockTimeSkewContext, DataQualityScore, WeatherObservationPoint, WeatherValidationPolicy,
    validate_weather_observation_with_temporal_context_elevation_and_block_time,
};
use rustc_hash::{FxHashMap, FxHashSet};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const APPLIED_REVEAL_RETENTION_BLOCKS: u64 = 10_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitRecord {
    pub commit_id: String,
    pub station_id: String,
    pub contribution_type: String,
    /// Snapshotted from canonical station state when the commit is accepted.
    /// Missing persisted values fail closed as `Unclassified`.
    #[serde(default)]
    pub provenance_class: ContributionProvenanceClass,
    pub committed_at: u64,
    pub commit_expires_at: u64,
    pub reveal_expires_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevealRecord {
    pub commit_id: String,
    #[serde(default)]
    pub station_id: String,
    /// Copied from the matching commit, never re-derived from free-form reveal
    /// payload data.
    #[serde(default)]
    pub provenance_class: ContributionProvenanceClass,
    pub revealed_at: u64,
    pub reveal_id: String,
    #[serde(default)]
    pub canonical_calibration_attestation_cid: Option<String>,
    #[serde(default)]
    pub quality_score: Option<DataQualityScore>,
}

#[derive(Debug, Clone)]
pub enum CommitRevealTransition {
    RegisterCommit {
        commit_id: String,
        committed_at: u64,
        commit_expires_at: u64,
        reveal_expires_at: u64,
    },
    ApplyReveal {
        commit_id: String,
        revealed_at: u64,
    },
}

/// Result of [`CommitRevealState::apply_reveal_with_block_time`].
#[derive(Debug, Clone)]
pub struct AppliedReveal {
    pub transition: CommitRevealTransition,
    /// Whether this reveal carried a weather payload and therefore replaced
    /// the station's last accepted reading (the baseline the next reading's
    /// temporal checks compare against).
    pub weather_point_recorded: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CommitRevealError {
    #[error("commit_id is required")]
    MissingCommitId,
    #[error("duplicate commit_id")]
    DuplicateCommitId,
    #[error("reveal does not match commit hash")]
    RevealCommitHashMismatch,
    #[error("invalid CID fields: {0}")]
    InvalidCidFields(RevealCidValidationError),
    #[error("invalid provenance fields: {0}")]
    InvalidProvenanceFields(RevealProvenanceValidationError),
    #[error("reveal payload serialization failed")]
    RevealPayloadSerializationFailed,
    #[error("weather payload schema validation failed: {reason}")]
    WeatherPayloadSchemaValidation { reason: String },
    #[error("weather observation validation failed: {reason}")]
    WeatherObservationValidation { reason: String },
    #[error("reveal replay detected")]
    RevealReplayDetected,
    #[error(
        "calibration attestation CID replay detected: canonical CID already bound to station '{existing_station_id}', received '{attempted_station_id}'"
    )]
    CalibrationAttestationCidReplay {
        existing_station_id: String,
        attempted_station_id: String,
    },
    #[error("unknown commit for reveal")]
    UnknownCommitForReveal,
    #[error("reveal cannot precede commit")]
    RevealPrecedesCommit,
    #[error("commit has expired")]
    CommitExpired,
    #[error("reveal window has expired")]
    RevealWindowExpired,
    #[error("reveal metadata mismatch")]
    RevealMetadataMismatch,
    #[error("forecast reveal requires manifest")]
    ForecastManifestRequired,
    #[error("invalid forecast manifest: {0}")]
    InvalidForecastManifest(ForecastManifestValidationError),
    #[error("invalid forecast verification receipt: {0}")]
    InvalidForecastReceipt(ForecastReceiptValidationError),
    #[error("receipt manifest_digest does not match manifest")]
    ReceiptManifestDigestMismatch,
    #[error("receipt commit_id does not match payload")]
    ReceiptCommitIdMismatch,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct CommitRevealState {
    pub pending_commits: FxHashMap<String, CommitRecord>,
    pub applied_reveals: FxHashMap<String, RevealRecord>,
    #[serde(default)]
    station_last_weather: FxHashMap<String, WeatherObservationPoint>,
    #[serde(default)]
    weather_validation_policy: WeatherValidationPolicy,
    #[serde(default)]
    weather_payload_parse_options: WeatherPayloadParseOptions,
    replay_keys: FxHashSet<String>,
    #[serde(default)]
    calibration_cid_bindings: FxHashMap<String, String>,
}

impl CommitRevealState {
    pub fn new_with_weather_policy(weather_validation_policy: WeatherValidationPolicy) -> Self {
        Self {
            weather_validation_policy,
            ..Self::default()
        }
    }

    pub fn weather_validation_policy(&self) -> &WeatherValidationPolicy {
        &self.weather_validation_policy
    }

    pub fn set_weather_validation_policy(&mut self, policy: WeatherValidationPolicy) {
        self.weather_validation_policy = policy;
    }

    pub fn weather_payload_parse_options(&self) -> WeatherPayloadParseOptions {
        self.weather_payload_parse_options
    }

    pub fn set_weather_payload_parse_options(&mut self, options: WeatherPayloadParseOptions) {
        self.weather_payload_parse_options = options;
    }

    pub fn register_commit(
        &mut self,
        payload: &CommitPayload,
        current_block_height: u64,
    ) -> Result<CommitRevealTransition, CommitRevealError> {
        self.register_commit_with_provenance(
            payload,
            current_block_height,
            ContributionProvenanceClass::Unclassified,
        )
    }

    pub fn register_commit_with_provenance(
        &mut self,
        payload: &CommitPayload,
        current_block_height: u64,
        provenance_class: ContributionProvenanceClass,
    ) -> Result<CommitRevealTransition, CommitRevealError> {
        if payload.commit_id.is_empty() {
            return Err(CommitRevealError::MissingCommitId);
        }

        if self.pending_commits.contains_key(&payload.commit_id)
            || self.applied_reveals.contains_key(&payload.commit_id)
        {
            return Err(CommitRevealError::DuplicateCommitId);
        }

        let commit_expires_at =
            current_block_height.saturating_add(commit_window_blocks(current_block_height));
        let reveal_expires_at =
            current_block_height.saturating_add(reveal_window_blocks(current_block_height));

        self.pending_commits.insert(
            payload.commit_id.clone(),
            CommitRecord {
                commit_id: payload.commit_id.clone(),
                station_id: payload.station_id.clone(),
                contribution_type: payload.contribution_type.clone(),
                provenance_class,
                committed_at: current_block_height,
                commit_expires_at,
                reveal_expires_at,
            },
        );

        Ok(CommitRevealTransition::RegisterCommit {
            commit_id: payload.commit_id.clone(),
            committed_at: current_block_height,
            commit_expires_at,
            reveal_expires_at,
        })
    }

    pub fn apply_reveal(
        &mut self,
        payload: &RevealPayload,
        current_block_height: u64,
    ) -> Result<CommitRevealTransition, CommitRevealError> {
        self.apply_reveal_with_elevation(payload, current_block_height, None)
    }

    pub fn apply_reveal_with_elevation(
        &mut self,
        payload: &RevealPayload,
        current_block_height: u64,
        elevation_m: Option<f64>,
    ) -> Result<CommitRevealTransition, CommitRevealError> {
        self.apply_reveal_with_block_time(payload, current_block_height, elevation_m, None)
            .map(|applied| applied.transition)
    }

    /// The station's last accepted weather reading, if any.
    pub fn station_last_weather(&self, station_id: &str) -> Option<&WeatherObservationPoint> {
        self.station_last_weather.get(station_id)
    }

    /// [`Self::apply_reveal_with_elevation`] with the optional #3270
    /// block-time skew context. `None` is the pre-activation rule, byte for
    /// byte; the caller resolves `Some` only when
    /// `ConsensusFeature::WeatherSkewBlockTimeAllowance` is active AND the accepting
    /// block of the station's previous reading is known.
    pub fn apply_reveal_with_block_time(
        &mut self,
        payload: &RevealPayload,
        current_block_height: u64,
        // determinism-ok: elevation passthrough, unchanged; #3270 adds only the block-time arg
        elevation_m: Option<f64>,
        block_time: Option<BlockTimeSkewContext>,
    ) -> Result<AppliedReveal, CommitRevealError> {
        let computed_commit = Self::compute_commit_id(payload)?;

        if computed_commit != payload.commit_id {
            return Err(CommitRevealError::RevealCommitHashMismatch);
        }

        payload
            .validate_cid_fields()
            .map_err(CommitRevealError::InvalidCidFields)?;
        payload
            .validate_provenance_fields()
            .map_err(CommitRevealError::InvalidProvenanceFields)?;

        if payload.contribution_type == "forecast" {
            let manifest = payload
                .manifest
                .as_ref()
                .ok_or(CommitRevealError::ForecastManifestRequired)?;
            manifest
                .validate()
                .map_err(CommitRevealError::InvalidForecastManifest)?;
            if let Some(receipt) = payload.verification_receipt.as_ref() {
                receipt
                    .validate()
                    .map_err(CommitRevealError::InvalidForecastReceipt)?;
                if receipt.manifest_digest != forecast_manifest_digest(manifest) {
                    return Err(CommitRevealError::ReceiptManifestDigestMismatch);
                }
                if receipt.commit_id != payload.commit_id {
                    return Err(CommitRevealError::ReceiptCommitIdMismatch);
                }
            }
        }

        let parsed_weather_payload =
            parse_weather_payload_with_options(&payload.data, self.weather_payload_parse_options)
                .map_err(|reason| CommitRevealError::WeatherPayloadSchemaValidation { reason })?;
        let weather_point = if let Some(parsed) = parsed_weather_payload {
            let previous = self.station_last_weather.get(&payload.station_id);
            let validation =
                validate_weather_observation_with_temporal_context_elevation_and_block_time(
                    &parsed.observation,
                    payload.timestamp,
                    current_block_height,
                    previous,
                    &self.weather_validation_policy,
                    elevation_m,
                    block_time,
                );
            if !validation.valid {
                return Err(CommitRevealError::WeatherObservationValidation {
                    reason: validation.errors.join("; "),
                });
            }
            Some((
                validation.score,
                WeatherObservationPoint {
                    observed_at_unix: payload.timestamp,
                    observed_at_block: current_block_height,
                    observation: parsed.observation,
                },
            ))
        } else {
            None
        };

        let reveal_id = Self::compute_reveal_id(payload)?;
        if self.replay_keys.contains(&reveal_id) {
            return Err(CommitRevealError::RevealReplayDetected);
        }
        let canonical_calibration_attestation_cid = payload
            .provenance
            .as_ref()
            .and_then(|provenance| provenance.calibration_attestation_cid.as_deref())
            .map(canonicalize_calibration_attestation_cid)
            .transpose()
            .map_err(CommitRevealError::InvalidProvenanceFields)?;
        if let Some(canonical_cid) = canonical_calibration_attestation_cid.as_deref() {
            if let Some(existing_station_id) =
                self.find_station_binding_for_calibration_cid(canonical_cid)
            {
                if existing_station_id != payload.station_id {
                    return Err(CommitRevealError::CalibrationAttestationCidReplay {
                        existing_station_id: existing_station_id.to_string(),
                        attempted_station_id: payload.station_id.clone(),
                    });
                }
            }
        }

        let Some(commit) = self.pending_commits.get(&payload.commit_id).cloned() else {
            return Err(CommitRevealError::UnknownCommitForReveal);
        };

        if current_block_height < commit.committed_at {
            return Err(CommitRevealError::RevealPrecedesCommit);
        }
        if current_block_height > commit.commit_expires_at {
            return Err(CommitRevealError::CommitExpired);
        }
        if current_block_height > commit.reveal_expires_at {
            return Err(CommitRevealError::RevealWindowExpired);
        }
        if payload.station_id != commit.station_id
            || payload.contribution_type != commit.contribution_type
        {
            return Err(CommitRevealError::RevealMetadataMismatch);
        }

        self.pending_commits.remove(&payload.commit_id);
        self.replay_keys.insert(reveal_id.clone());
        let quality_score = weather_point.as_ref().map(|(score, _)| *score);
        let weather_point_recorded = weather_point.is_some();
        if let Some((_, point)) = weather_point {
            self.station_last_weather
                .insert(payload.station_id.clone(), point);
        }
        if let Some(cid) = &canonical_calibration_attestation_cid {
            if !payload.station_id.is_empty() {
                self.calibration_cid_bindings
                    .insert(cid.clone(), payload.station_id.clone());
            }
        }
        self.applied_reveals.insert(
            payload.commit_id.clone(),
            RevealRecord {
                commit_id: payload.commit_id.clone(),
                station_id: payload.station_id.clone(),
                provenance_class: commit.provenance_class,
                revealed_at: current_block_height,
                reveal_id,
                canonical_calibration_attestation_cid,
                quality_score,
            },
        );

        Ok(AppliedReveal {
            transition: CommitRevealTransition::ApplyReveal {
                commit_id: payload.commit_id.clone(),
                revealed_at: current_block_height,
            },
            weather_point_recorded,
        })
    }

    pub fn prune_applied_reveals(&mut self, current_block_height: u64) {
        let cutoff = current_block_height.saturating_sub(APPLIED_REVEAL_RETENTION_BLOCKS);
        self.applied_reveals
            .retain(|_, record| record.revealed_at >= cutoff);
        self.replay_keys = self
            .applied_reveals
            .values()
            .map(|r| r.reveal_id.clone())
            .collect();
    }

    pub fn prune_expired(&mut self, current_block_height: u64) {
        self.pending_commits
            .retain(|_, record| current_block_height <= record.reveal_expires_at);
        self.prune_applied_reveals(current_block_height);
    }

    pub fn has_applied_reveal(&self, commit_id: &str) -> bool {
        self.applied_reveals.contains_key(commit_id)
    }

    pub fn calibration_cid_bindings(&self) -> &FxHashMap<String, String> {
        &self.calibration_cid_bindings
    }

    fn find_station_binding_for_calibration_cid(&self, canonical_cid: &str) -> Option<&str> {
        if let Some(station_id) = self.calibration_cid_bindings.get(canonical_cid) {
            return Some(station_id.as_str());
        }
        self.applied_reveals.values().find_map(|record| {
            let matches_cid =
                record.canonical_calibration_attestation_cid.as_deref() == Some(canonical_cid);
            (matches_cid && !record.station_id.is_empty()).then_some(record.station_id.as_str())
        })
    }

    fn compute_commit_id(payload: &RevealPayload) -> Result<String, CommitRevealError> {
        let data_str = serde_json::to_string(&payload.data)
            .map_err(|_| CommitRevealError::RevealPayloadSerializationFailed)?;
        let preimage = format!(
            "{}||{}||{}||{}||{}",
            data_str,
            payload.nonce,
            payload.station_id,
            payload.timestamp,
            payload.contribution_type
        );
        Ok(hash_bytes(preimage.as_bytes()).to_string())
    }

    fn compute_reveal_id(payload: &RevealPayload) -> Result<String, CommitRevealError> {
        let data_str = serde_json::to_string(&payload.data)
            .map_err(|_| CommitRevealError::RevealPayloadSerializationFailed)?;
        let preimage = format!(
            "{}||{}||{}||{}||{}",
            payload.commit_id,
            payload.station_id,
            payload.contribution_type,
            payload.timestamp,
            data_str
        );
        Ok(hash_bytes(preimage.as_bytes()).to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blockchain::transaction::{
        CommitPayload, RevealPayload, RevealProvenanceValidationError, SensorProvenance,
        Transaction,
    };
    use std::panic::{AssertUnwindSafe, catch_unwind};

    fn build_reveal(station_id: &str, contribution_type: &str, timestamp: i64) -> RevealPayload {
        let data = serde_json::json!({"temperature": 20.1, "humidity": 0.45});
        let nonce = "nonce-1".to_string();
        let commit_id =
            Transaction::compute_commit_id(&data, &nonce, station_id, timestamp, contribution_type);
        RevealPayload {
            commit_id,
            station_id: station_id.to_string(),
            timestamp,
            contribution_type: contribution_type.to_string(),
            data,
            nonce,
            data_cid: None,
            data_hash: None,
            storage_provider: None,
            provenance: None,
            manifest: None,
            verification_receipt: None,
            compute_receipt: None,
                quality_tier: None,
        }
    }

    fn build_reveal_with_cid(
        station_id: &str,
        contribution_type: &str,
        timestamp: i64,
    ) -> RevealPayload {
        let mut reveal = build_reveal(station_id, contribution_type, timestamp);
        reveal.data_cid = Some("QmYwAPJzv5CZsnN625s3Xf2nemtYgPpHdWEz79ojWnPbdG".to_string());
        reveal.data_hash =
            Some("a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2".to_string());
        reveal.storage_provider = Some("ipfs".to_string());
        reveal
    }

    fn build_reveal_with_calibration_attestation_cid(
        station_id: &str,
        contribution_type: &str,
        timestamp: i64,
        calibration_attestation_cid: &str,
    ) -> RevealPayload {
        let mut reveal = build_reveal(station_id, contribution_type, timestamp);
        reveal.provenance = Some(SensorProvenance {
            source_id: None,
            manufacturer: None,
            model: None,
            firmware_hash: None,
            calibration_attestation_cid: Some(calibration_attestation_cid.to_string()),
        });
        reveal
    }

    #[test]
    fn register_then_apply_reveal() {
        let mut state = CommitRevealState::default();
        let terminal_height = 10_000_000u64;
        let reveal = build_reveal("station-1", "contribution", 1_700_000_000);
        let commit = CommitPayload {
            commit_id: reveal.commit_id.clone(),
            station_id: reveal.station_id.clone(),
            timestamp: reveal.timestamp,
            contribution_type: reveal.contribution_type.clone(),
            auth_pubkey: None,
            auth_signature: None,
        };

        let reg = state
            .register_commit(&commit, terminal_height)
            .expect("register commit");
        match reg {
            CommitRevealTransition::RegisterCommit { .. } => {},
            _ => panic!("expected RegisterCommit transition"),
        }

        let applied = state
            .apply_reveal(&reveal, terminal_height + 10)
            .expect("apply reveal");
        match applied {
            CommitRevealTransition::ApplyReveal { .. } => {},
            _ => panic!("expected ApplyReveal transition"),
        }

        assert!(state.has_applied_reveal(&reveal.commit_id));
    }

    #[test]
    fn reject_reveal_replay() {
        let mut state = CommitRevealState::default();
        let reveal = build_reveal("station-1", "contribution", 1_700_000_000);
        let commit = CommitPayload {
            commit_id: reveal.commit_id.clone(),
            station_id: reveal.station_id.clone(),
            timestamp: reveal.timestamp,
            contribution_type: reveal.contribution_type.clone(),
            auth_pubkey: None,
            auth_signature: None,
        };

        state.register_commit(&commit, 1).expect("register");
        state.apply_reveal(&reveal, 2).expect("first reveal");

        let err = state
            .apply_reveal(&reveal, 3)
            .expect_err("second reveal should be rejected");
        assert!(err.to_string().contains("reveal replay") || err.to_string().contains("unknown"));
    }

    #[test]
    fn reject_expired_reveal() {
        let mut state = CommitRevealState::default();
        let reveal = build_reveal("station-1", "contribution", 1_700_000_000);
        let commit = CommitPayload {
            commit_id: reveal.commit_id.clone(),
            station_id: reveal.station_id.clone(),
            timestamp: reveal.timestamp,
            contribution_type: reveal.contribution_type.clone(),
            auth_pubkey: None,
            auth_signature: None,
        };

        state.register_commit(&commit, 1).expect("register");
        // At height 1, reveal_window_blocks(1) returns the genesis-era window.
        let window = reveal_window_blocks(1);
        let err = state
            .apply_reveal(&reveal, 1 + window + 1)
            .expect_err("expired reveal should be rejected");
        assert!(err.to_string().contains("expired"));
    }

    #[test]
    fn reveal_within_genesis_window_is_accepted() {
        let mut state = CommitRevealState::default();
        let reveal = build_reveal("station-1", "contribution", 1_700_000_000);
        let commit = CommitPayload {
            commit_id: reveal.commit_id.clone(),
            station_id: reveal.station_id.clone(),
            timestamp: reveal.timestamp,
            contribution_type: reveal.contribution_type.clone(),
            auth_pubkey: None,
            auth_signature: None,
        };

        state.register_commit(&commit, 1).expect("register");
        let window = reveal_window_blocks(1);
        state
            .apply_reveal(&reveal, 1 + window)
            .expect("reveal at the genesis-era deadline should be accepted");
    }

    #[test]
    fn reveal_with_valid_cid_accepted() {
        let mut state = CommitRevealState::default();
        let terminal_height = 10_000_000u64;
        let reveal = build_reveal_with_cid("station-1", "contribution", 1_700_000_000);
        let commit = CommitPayload {
            commit_id: reveal.commit_id.clone(),
            station_id: reveal.station_id.clone(),
            timestamp: reveal.timestamp,
            contribution_type: reveal.contribution_type.clone(),
            auth_pubkey: None,
            auth_signature: None,
        };

        state
            .register_commit(&commit, terminal_height)
            .expect("register commit");
        state
            .apply_reveal(&reveal, terminal_height + 10)
            .expect("CID reveal should be accepted");
        assert!(state.has_applied_reveal(&reveal.commit_id));
    }

    #[test]
    fn reveal_with_partial_cid_rejected() {
        let mut state = CommitRevealState::default();
        let mut reveal = build_reveal("station-1", "contribution", 1_700_000_000);
        reveal.data_cid = Some("QmYwAPJzv5CZsnN625s3Xf2nemtYgPpHdWEz79ojWnPbdG".to_string());
        // data_hash and storage_provider left as None

        let commit = CommitPayload {
            commit_id: reveal.commit_id.clone(),
            station_id: reveal.station_id.clone(),
            timestamp: reveal.timestamp,
            contribution_type: reveal.contribution_type.clone(),
            auth_pubkey: None,
            auth_signature: None,
        };

        state.register_commit(&commit, 10).expect("register commit");
        let err = state
            .apply_reveal(&reveal, 20)
            .expect_err("partial CID fields should be rejected");
        assert!(matches!(
            err,
            CommitRevealError::InvalidCidFields(RevealCidValidationError::CidFieldsPartial)
        ));
    }

    #[test]
    fn reveal_with_invalid_cid_hash_rejected() {
        let mut state = CommitRevealState::default();
        let mut reveal = build_reveal_with_cid("station-1", "contribution", 1_700_000_000);
        reveal.data_hash = Some("not-a-hex-hash".to_string());

        let commit = CommitPayload {
            commit_id: reveal.commit_id.clone(),
            station_id: reveal.station_id.clone(),
            timestamp: reveal.timestamp,
            contribution_type: reveal.contribution_type.clone(),
            auth_pubkey: None,
            auth_signature: None,
        };

        state.register_commit(&commit, 10).expect("register commit");
        let err = state
            .apply_reveal(&reveal, 20)
            .expect_err("invalid hash format should be rejected");
        assert!(matches!(
            err,
            CommitRevealError::InvalidCidFields(RevealCidValidationError::InvalidDataHashFormat)
        ));
    }

    #[test]
    fn reveal_with_invalid_provider_rejected() {
        let mut state = CommitRevealState::default();
        let mut reveal = build_reveal_with_cid("station-1", "contribution", 1_700_000_000);
        reveal.storage_provider = Some("s3".to_string());

        let commit = CommitPayload {
            commit_id: reveal.commit_id.clone(),
            station_id: reveal.station_id.clone(),
            timestamp: reveal.timestamp,
            contribution_type: reveal.contribution_type.clone(),
            auth_pubkey: None,
            auth_signature: None,
        };

        state.register_commit(&commit, 10).expect("register commit");
        let err = state
            .apply_reveal(&reveal, 20)
            .expect_err("invalid provider should be rejected");
        assert!(matches!(
            err,
            CommitRevealError::InvalidCidFields(RevealCidValidationError::InvalidStorageProvider)
        ));
    }

    #[test]
    fn reveal_with_malformed_cid_format_rejected() {
        let mut state = CommitRevealState::default();
        let mut reveal = build_reveal_with_cid("station-1", "contribution", 1_700_000_000);
        reveal.data_cid = Some("QmNotAValidCIDBecauseContains0".to_string());

        let commit = CommitPayload {
            commit_id: reveal.commit_id.clone(),
            station_id: reveal.station_id.clone(),
            timestamp: reveal.timestamp,
            contribution_type: reveal.contribution_type.clone(),
            auth_pubkey: None,
            auth_signature: None,
        };

        state.register_commit(&commit, 10).expect("register commit");
        let err = state
            .apply_reveal(&reveal, 20)
            .expect_err("malformed CID should be rejected");
        assert!(matches!(
            err,
            CommitRevealError::InvalidCidFields(RevealCidValidationError::InvalidIpfsLikeCidFormat)
        ));
    }

    #[test]
    fn reveal_with_empty_provenance_object_rejected() {
        let mut state = CommitRevealState::default();
        let mut reveal = build_reveal("station-prov", "contribution", 1_700_000_200);
        reveal.provenance = Some(SensorProvenance {
            source_id: None,
            manufacturer: None,
            model: None,
            firmware_hash: None,
            calibration_attestation_cid: None,
        });

        let commit = CommitPayload {
            commit_id: reveal.commit_id.clone(),
            station_id: reveal.station_id.clone(),
            timestamp: reveal.timestamp,
            contribution_type: reveal.contribution_type.clone(),
            auth_pubkey: None,
            auth_signature: None,
        };

        state.register_commit(&commit, 10).expect("register commit");
        let err = state
            .apply_reveal(&reveal, 20)
            .expect_err("empty provenance object should be rejected");
        assert!(matches!(
            err,
            CommitRevealError::InvalidProvenanceFields(
                RevealProvenanceValidationError::EmptyProvenance
            )
        ));
    }

    #[test]
    fn calibration_attestation_cid_replay_rejected_for_different_station() {
        let mut state = CommitRevealState::default();
        let first = build_reveal_with_calibration_attestation_cid(
            "station-a",
            "contribution",
            1_700_000_201,
            "ipfs://QmYwAPJzv5CZsnN625s3Xf2nemtYgPpHdWEz79ojWnPbdG",
        );
        let first_commit = CommitPayload {
            commit_id: first.commit_id.clone(),
            station_id: first.station_id.clone(),
            timestamp: first.timestamp,
            contribution_type: first.contribution_type.clone(),
            auth_pubkey: None,
            auth_signature: None,
        };
        state
            .register_commit(&first_commit, 30)
            .expect("register first commit");
        state
            .apply_reveal(&first, 31)
            .expect("first reveal should bind canonical CID");

        let second = build_reveal_with_calibration_attestation_cid(
            "station-b",
            "contribution",
            1_700_000_202,
            "QmYwAPJzv5CZsnN625s3Xf2nemtYgPpHdWEz79ojWnPbdG",
        );
        let second_commit = CommitPayload {
            commit_id: second.commit_id.clone(),
            station_id: second.station_id.clone(),
            timestamp: second.timestamp,
            contribution_type: second.contribution_type.clone(),
            auth_pubkey: None,
            auth_signature: None,
        };
        state
            .register_commit(&second_commit, 32)
            .expect("register second commit");

        let err = state
            .apply_reveal(&second, 33)
            .expect_err("cross-station replay of canonical CID must fail");
        assert!(matches!(
            err,
            CommitRevealError::CalibrationAttestationCidReplay { .. }
        ));
    }

    #[test]
    fn calibration_attestation_cid_reuse_allowed_for_same_station_across_equivalent_forms() {
        let mut state = CommitRevealState::default();
        let first = build_reveal_with_calibration_attestation_cid(
            "station-a",
            "contribution",
            1_700_000_203,
            "ipfs://QmYwAPJzv5CZsnN625s3Xf2nemtYgPpHdWEz79ojWnPbdG",
        );
        let first_commit = CommitPayload {
            commit_id: first.commit_id.clone(),
            station_id: first.station_id.clone(),
            timestamp: first.timestamp,
            contribution_type: first.contribution_type.clone(),
            auth_pubkey: None,
            auth_signature: None,
        };
        state
            .register_commit(&first_commit, 40)
            .expect("register first commit");
        state.apply_reveal(&first, 41).expect("apply first reveal");

        let second = build_reveal_with_calibration_attestation_cid(
            "station-a",
            "contribution",
            1_700_000_204,
            "/ipfs/QmYwAPJzv5CZsnN625s3Xf2nemtYgPpHdWEz79ojWnPbdG",
        );
        let second_commit = CommitPayload {
            commit_id: second.commit_id.clone(),
            station_id: second.station_id.clone(),
            timestamp: second.timestamp,
            contribution_type: second.contribution_type.clone(),
            auth_pubkey: None,
            auth_signature: None,
        };
        state
            .register_commit(&second_commit, 42)
            .expect("register second commit");
        state
            .apply_reveal(&second, 43)
            .expect("same-station canonical CID reuse should be allowed");
    }

    #[test]
    fn malformed_calibration_attestation_cid_takes_precedence_over_replay_check() {
        let mut state = CommitRevealState::default();
        let first = build_reveal_with_calibration_attestation_cid(
            "station-a",
            "contribution",
            1_700_000_205,
            "ipfs://QmYwAPJzv5CZsnN625s3Xf2nemtYgPpHdWEz79ojWnPbdG",
        );
        let first_commit = CommitPayload {
            commit_id: first.commit_id.clone(),
            station_id: first.station_id.clone(),
            timestamp: first.timestamp,
            contribution_type: first.contribution_type.clone(),
            auth_pubkey: None,
            auth_signature: None,
        };
        state
            .register_commit(&first_commit, 50)
            .expect("register first commit");
        state.apply_reveal(&first, 51).expect("apply first reveal");

        let second = build_reveal_with_calibration_attestation_cid(
            "station-b",
            "contribution",
            1_700_000_206,
            "ipfs://QmNotAValidCIDBecauseContains0",
        );
        let second_commit = CommitPayload {
            commit_id: second.commit_id.clone(),
            station_id: second.station_id.clone(),
            timestamp: second.timestamp,
            contribution_type: second.contribution_type.clone(),
            auth_pubkey: None,
            auth_signature: None,
        };
        state
            .register_commit(&second_commit, 52)
            .expect("register second commit");

        let err = state
            .apply_reveal(&second, 53)
            .expect_err("malformed CID should fail before replay binding checks");
        assert!(matches!(
            err,
            CommitRevealError::InvalidProvenanceFields(
                RevealProvenanceValidationError::InvalidCalibrationAttestationCid
            )
        ));
    }

    #[test]
    fn malformed_cid_input_fails_without_panicking() {
        let mut state = CommitRevealState::default();
        let mut reveal = build_reveal_with_cid("station-unsafe", "contribution", 1_700_000_100);
        reveal.data_cid = Some(format!(
            "QmYwAPJzv5CZsnN625s3Xf2nemtYgP{}\u{0007}HdWEz79ojWnPbdG",
            "A".repeat(220)
        ));

        let commit = CommitPayload {
            commit_id: reveal.commit_id.clone(),
            station_id: reveal.station_id.clone(),
            timestamp: reveal.timestamp,
            contribution_type: reveal.contribution_type.clone(),
            auth_pubkey: None,
            auth_signature: None,
        };
        state.register_commit(&commit, 10).expect("register commit");

        let panic_result = catch_unwind(AssertUnwindSafe(|| state.apply_reveal(&reveal, 20)));
        assert!(
            panic_result.is_ok(),
            "apply_reveal should never panic on malformed CID input"
        );

        let result = panic_result.expect("panic result should be available");
        let err = result.expect_err("malformed CID input must return an error");
        assert!(matches!(
            err,
            CommitRevealError::InvalidCidFields(RevealCidValidationError::CidTooLong)
        ));
    }

    #[test]
    fn cid_mode_preserves_commit_verification() {
        let mut state = CommitRevealState::default();
        let mut reveal = build_reveal_with_cid("station-1", "contribution", 1_700_000_000);
        // Corrupt commit_id to verify commit hash check still runs before CID validation
        reveal.commit_id = "bad_commit_id".to_string();

        let commit = CommitPayload {
            commit_id: reveal.commit_id.clone(),
            station_id: reveal.station_id.clone(),
            timestamp: reveal.timestamp,
            contribution_type: reveal.contribution_type.clone(),
            auth_pubkey: None,
            auth_signature: None,
        };

        state.register_commit(&commit, 10).expect("register commit");
        let err = state
            .apply_reveal(&reveal, 20)
            .expect_err("mismatched commit hash should be rejected even with CID");
        assert!(err.to_string().contains("does not match commit hash"));
    }

    #[test]
    fn weather_reveal_with_impossible_readings_rejected() {
        let mut state = CommitRevealState::default();
        let mut reveal = build_reveal("station-1", "contribution", 1_700_000_000);
        reveal.data = serde_json::json!({
            "temperature": 120.0,
            "humidity": 150.0,
            "pressure": 500.0
        });
        reveal.commit_id = Transaction::compute_commit_id(
            &reveal.data,
            &reveal.nonce,
            &reveal.station_id,
            reveal.timestamp,
            &reveal.contribution_type,
        );
        let commit = CommitPayload {
            commit_id: reveal.commit_id.clone(),
            station_id: reveal.station_id.clone(),
            timestamp: reveal.timestamp,
            contribution_type: reveal.contribution_type.clone(),
            auth_pubkey: None,
            auth_signature: None,
        };

        state.register_commit(&commit, 10).expect("register commit");
        let err = state
            .apply_reveal(&reveal, 20)
            .expect_err("impossible weather readings should be rejected");
        assert!(
            err.to_string()
                .contains("weather observation validation failed")
        );
    }

    #[test]
    fn weather_reveal_quality_score_is_stored() {
        let mut state = CommitRevealState::default();
        let terminal_height = 10_000_000u64;
        let mut reveal = build_reveal("station-2", "contribution", 1_700_000_111);
        reveal.data = serde_json::json!({
            "temperature": 20.0,
            "humidity": 60.0,
            "pressure": 1007.0,
            "dewpoint": 12.0
        });
        reveal.commit_id = Transaction::compute_commit_id(
            &reveal.data,
            &reveal.nonce,
            &reveal.station_id,
            reveal.timestamp,
            &reveal.contribution_type,
        );
        let commit = CommitPayload {
            commit_id: reveal.commit_id.clone(),
            station_id: reveal.station_id.clone(),
            timestamp: reveal.timestamp,
            contribution_type: reveal.contribution_type.clone(),
            auth_pubkey: None,
            auth_signature: None,
        };

        state
            .register_commit(&commit, terminal_height)
            .expect("register commit");
        state
            .apply_reveal(&reveal, terminal_height + 10)
            .expect("valid weather reveal");
        let stored = state.applied_reveals.get(&reveal.commit_id).unwrap();
        assert!(stored.quality_score.is_some());
        assert!(stored.quality_score.as_ref().unwrap().overall > 0.8);
    }

    #[test]
    fn weather_reveal_rejects_stale_station_timestamp() {
        let mut state = CommitRevealState::default();
        let terminal_height = 10_000_000u64;

        let mut first = build_reveal("station-3", "contribution", 1_700_000_500);
        first.data = serde_json::json!({
            "temperature": 20.0,
            "humidity": 60.0,
            "pressure": 1007.0,
            "dewpoint": 12.0
        });
        first.commit_id = Transaction::compute_commit_id(
            &first.data,
            &first.nonce,
            &first.station_id,
            first.timestamp,
            &first.contribution_type,
        );
        let first_commit = CommitPayload {
            commit_id: first.commit_id.clone(),
            station_id: first.station_id.clone(),
            timestamp: first.timestamp,
            contribution_type: first.contribution_type.clone(),
            auth_pubkey: None,
            auth_signature: None,
        };

        state
            .register_commit(&first_commit, terminal_height)
            .expect("register first");
        state
            .apply_reveal(&first, terminal_height + 2)
            .expect("apply first");

        let mut stale = build_reveal("station-3", "contribution", 1_700_000_400);
        stale.data = serde_json::json!({
            "temperature": 20.1,
            "humidity": 61.0,
            "pressure": 1007.1,
            "dewpoint": 12.1
        });
        stale.commit_id = Transaction::compute_commit_id(
            &stale.data,
            &stale.nonce,
            &stale.station_id,
            stale.timestamp,
            &stale.contribution_type,
        );
        let stale_commit = CommitPayload {
            commit_id: stale.commit_id.clone(),
            station_id: stale.station_id.clone(),
            timestamp: stale.timestamp,
            contribution_type: stale.contribution_type.clone(),
            auth_pubkey: None,
            auth_signature: None,
        };
        state
            .register_commit(&stale_commit, terminal_height + 3)
            .expect("register stale");
        let err = state
            .apply_reveal(&stale, terminal_height + 4)
            .expect_err("stale timestamp should be rejected");
        assert!(err.to_string().contains("stale weather timestamp"));
    }

    #[test]
    fn weather_reveal_rejects_impossible_rate_change() {
        let mut state = CommitRevealState::default();

        let mut first = build_reveal("station-4", "contribution", 1_700_000_000);
        first.data = serde_json::json!({
            "temperature": 20.0,
            "humidity": 45.0,
            "pressure": 1008.0,
            "wind_speed": 4.0,
            "dewpoint": 10.0
        });
        first.commit_id = Transaction::compute_commit_id(
            &first.data,
            &first.nonce,
            &first.station_id,
            first.timestamp,
            &first.contribution_type,
        );
        let first_commit = CommitPayload {
            commit_id: first.commit_id.clone(),
            station_id: first.station_id.clone(),
            timestamp: first.timestamp,
            contribution_type: first.contribution_type.clone(),
            auth_pubkey: None,
            auth_signature: None,
        };
        state
            .register_commit(&first_commit, 20)
            .expect("register first");
        state.apply_reveal(&first, 21).expect("apply first");

        let mut second = build_reveal("station-4", "contribution", 1_700_000_010);
        second.data = serde_json::json!({
            "temperature": 35.0,
            "humidity": 90.0,
            "pressure": 990.0,
            "wind_speed": 30.0,
            "dewpoint": 26.0
        });
        second.commit_id = Transaction::compute_commit_id(
            &second.data,
            &second.nonce,
            &second.station_id,
            second.timestamp,
            &second.contribution_type,
        );
        let second_commit = CommitPayload {
            commit_id: second.commit_id.clone(),
            station_id: second.station_id.clone(),
            timestamp: second.timestamp,
            contribution_type: second.contribution_type.clone(),
            auth_pubkey: None,
            auth_signature: None,
        };
        state
            .register_commit(&second_commit, 22)
            .expect("register second");
        let err = state
            .apply_reveal(&second, 23)
            .expect_err("impossible rate should be rejected");
        assert!(err.to_string().contains("changed too quickly"));
    }

    #[test]
    fn weather_reveal_accepts_fast_but_plausible_transition() {
        let mut state = CommitRevealState::default();

        let mut first = build_reveal("station-5", "contribution", 1_700_001_000);
        first.data = serde_json::json!({
            "temperature": 22.0,
            "humidity": 70.0,
            "pressure": 1006.0,
            "wind_speed": 8.0,
            "precipitation": 1.0,
            "dewpoint": 16.0
        });
        first.commit_id = Transaction::compute_commit_id(
            &first.data,
            &first.nonce,
            &first.station_id,
            first.timestamp,
            &first.contribution_type,
        );
        let first_commit = CommitPayload {
            commit_id: first.commit_id.clone(),
            station_id: first.station_id.clone(),
            timestamp: first.timestamp,
            contribution_type: first.contribution_type.clone(),
            auth_pubkey: None,
            auth_signature: None,
        };
        state
            .register_commit(&first_commit, 30)
            .expect("register first");
        state.apply_reveal(&first, 31).expect("apply first");

        let mut second = build_reveal("station-5", "contribution", 1_700_001_120);
        second.data = serde_json::json!({
            "temperature": 26.0,
            "humidity": 78.0,
            "pressure": 1004.5,
            "wind_speed": 12.0,
            "precipitation": 3.0,
            "dewpoint": 18.0
        });
        second.commit_id = Transaction::compute_commit_id(
            &second.data,
            &second.nonce,
            &second.station_id,
            second.timestamp,
            &second.contribution_type,
        );
        let second_commit = CommitPayload {
            commit_id: second.commit_id.clone(),
            station_id: second.station_id.clone(),
            timestamp: second.timestamp,
            contribution_type: second.contribution_type.clone(),
            auth_pubkey: None,
            auth_signature: None,
        };
        state
            .register_commit(&second_commit, 32)
            .expect("register second");
        state
            .apply_reveal(&second, 33)
            .expect("plausible transition should be accepted");
    }

    #[test]
    fn weather_reveal_respects_custom_temporal_policy() {
        let strict_policy = WeatherValidationPolicy {
            max_temp_delta_per_second: 0.001,
            ..WeatherValidationPolicy::default()
        };
        let mut state = CommitRevealState::new_with_weather_policy(strict_policy);

        let mut first = build_reveal("station-6", "contribution", 1_700_002_000);
        first.data = serde_json::json!({
            "temperature": 20.0,
            "humidity": 65.0,
            "pressure": 1007.0,
            "dewpoint": 13.0
        });
        first.commit_id = Transaction::compute_commit_id(
            &first.data,
            &first.nonce,
            &first.station_id,
            first.timestamp,
            &first.contribution_type,
        );
        let first_commit = CommitPayload {
            commit_id: first.commit_id.clone(),
            station_id: first.station_id.clone(),
            timestamp: first.timestamp,
            contribution_type: first.contribution_type.clone(),
            auth_pubkey: None,
            auth_signature: None,
        };
        state
            .register_commit(&first_commit, 40)
            .expect("register first");
        state.apply_reveal(&first, 41).expect("apply first");

        let mut second = build_reveal("station-6", "contribution", 1_700_002_060);
        second.data = serde_json::json!({
            "temperature": 20.3,
            "humidity": 65.2,
            "pressure": 1007.1,
            "dewpoint": 13.2
        });
        second.commit_id = Transaction::compute_commit_id(
            &second.data,
            &second.nonce,
            &second.station_id,
            second.timestamp,
            &second.contribution_type,
        );
        let second_commit = CommitPayload {
            commit_id: second.commit_id.clone(),
            station_id: second.station_id.clone(),
            timestamp: second.timestamp,
            contribution_type: second.contribution_type.clone(),
            auth_pubkey: None,
            auth_signature: None,
        };
        state
            .register_commit(&second_commit, 42)
            .expect("register second");
        let err = state
            .apply_reveal(&second, 43)
            .expect_err("strict temp delta policy should reject second reveal");
        assert!(err.to_string().contains("temperature changed too quickly"));
    }

    #[test]
    fn weather_reveal_rejects_unknown_schema_version() {
        let mut state = CommitRevealState::default();
        let mut reveal = build_reveal("station-7", "contribution", 1_700_100_000);
        reveal.data = serde_json::json!({
            "schema_version": "wrf.weather.observation.v2",
            "observation": {
                "temperature_c": 20.0
            }
        });
        reveal.commit_id = Transaction::compute_commit_id(
            &reveal.data,
            &reveal.nonce,
            &reveal.station_id,
            reveal.timestamp,
            &reveal.contribution_type,
        );
        let commit = CommitPayload {
            commit_id: reveal.commit_id.clone(),
            station_id: reveal.station_id.clone(),
            timestamp: reveal.timestamp,
            contribution_type: reveal.contribution_type.clone(),
            auth_pubkey: None,
            auth_signature: None,
        };

        state.register_commit(&commit, 10).expect("register commit");
        let err = state
            .apply_reveal(&reveal, 11)
            .expect_err("unknown schema version should be rejected");
        assert!(
            err.to_string()
                .contains("weather payload schema validation failed")
        );
    }

    #[test]
    fn weather_reveal_accepts_canonical_v1_schema_payload() {
        let mut state = CommitRevealState::default();
        let mut reveal = build_reveal("station-8", "contribution", 1_700_110_000);
        reveal.data = serde_json::json!({
            "schema_version": "wrf.weather.observation.v1",
            "station_id": "station-8",
            "observed_at_unix": 1_700_110_000i64,
            "observation": {
                "temperature_c": 19.5,
                "humidity": 66.0,
                "pressure_hpa": 1008.2,
                "dewpoint_c": 13.0
            }
        });
        reveal.commit_id = Transaction::compute_commit_id(
            &reveal.data,
            &reveal.nonce,
            &reveal.station_id,
            reveal.timestamp,
            &reveal.contribution_type,
        );
        let commit = CommitPayload {
            commit_id: reveal.commit_id.clone(),
            station_id: reveal.station_id.clone(),
            timestamp: reveal.timestamp,
            contribution_type: reveal.contribution_type.clone(),
            auth_pubkey: None,
            auth_signature: None,
        };

        state.register_commit(&commit, 15).expect("register commit");
        state
            .apply_reveal(&reveal, 16)
            .expect("canonical v1 payload should pass");
        assert!(state.has_applied_reveal(&reveal.commit_id));
    }

    #[test]
    fn weather_reveal_rejects_non_string_schema_marker() {
        let mut state = CommitRevealState::default();
        let mut reveal = build_reveal("station-9", "contribution", 1_700_120_000);
        reveal.data = serde_json::json!({
            "schema_version": 1,
            "observation": {
                "temperature_c": 20.0
            }
        });
        reveal.commit_id = Transaction::compute_commit_id(
            &reveal.data,
            &reveal.nonce,
            &reveal.station_id,
            reveal.timestamp,
            &reveal.contribution_type,
        );
        let commit = CommitPayload {
            commit_id: reveal.commit_id.clone(),
            station_id: reveal.station_id.clone(),
            timestamp: reveal.timestamp,
            contribution_type: reveal.contribution_type.clone(),
            auth_pubkey: None,
            auth_signature: None,
        };

        state.register_commit(&commit, 10).expect("register commit");
        let err = state
            .apply_reveal(&reveal, 11)
            .expect_err("non-string schema marker should be rejected");
        assert!(
            err.to_string()
                .contains("weather payload schema validation failed")
        );
        assert!(err.to_string().contains("schema version marker"));
    }

    #[test]
    fn weather_reveal_strict_v1_keys_rejects_alias_only_observation() {
        let mut state = CommitRevealState::default();
        state.set_weather_payload_parse_options(WeatherPayloadParseOptions::strict_v1_keys());

        let mut reveal = build_reveal("station-10", "contribution", 1_700_130_000);
        reveal.data = serde_json::json!({
            "schema_version": "wrf.weather.observation.v1",
            "observation": {
                "temp_c": 19.5,
                "relative_humidity": 66.0,
                "pressure": 1008.2
            }
        });
        reveal.commit_id = Transaction::compute_commit_id(
            &reveal.data,
            &reveal.nonce,
            &reveal.station_id,
            reveal.timestamp,
            &reveal.contribution_type,
        );
        let commit = CommitPayload {
            commit_id: reveal.commit_id.clone(),
            station_id: reveal.station_id.clone(),
            timestamp: reveal.timestamp,
            contribution_type: reveal.contribution_type.clone(),
            auth_pubkey: None,
            auth_signature: None,
        };

        state.register_commit(&commit, 10).expect("register commit");
        let err = state
            .apply_reveal(&reveal, 11)
            .expect_err("strict mode should reject alias-only v1 payload");
        assert!(
            err.to_string()
                .contains("recognized canonical weather field")
        );
    }

    #[test]
    fn weather_reveal_default_v1_mode_accepts_alias_observation_for_migration() {
        let mut state = CommitRevealState::default();

        let mut reveal = build_reveal("station-11", "contribution", 1_700_140_000);
        reveal.data = serde_json::json!({
            "schema_version": "wrf.weather.observation.v1",
            "observation": {
                "temp_c": 19.5,
                "relative_humidity": 66.0,
                "pressure": 1008.2
            }
        });
        reveal.commit_id = Transaction::compute_commit_id(
            &reveal.data,
            &reveal.nonce,
            &reveal.station_id,
            reveal.timestamp,
            &reveal.contribution_type,
        );
        let commit = CommitPayload {
            commit_id: reveal.commit_id.clone(),
            station_id: reveal.station_id.clone(),
            timestamp: reveal.timestamp,
            contribution_type: reveal.contribution_type.clone(),
            auth_pubkey: None,
            auth_signature: None,
        };

        state.register_commit(&commit, 10).expect("register commit");
        state
            .apply_reveal(&reveal, 11)
            .expect("default mode should allow migration aliases");
    }

    /// Regression test for issue #703: 1000 commits + 100 reveals must complete
    /// in <100ms with batch pruning (was O(n*m) with per-tx pruning).
    #[test]
    fn commit_reveal_1000_commits_100_reveals_under_100ms() {
        let mut state = CommitRevealState::default();
        let block_height = 10u64;
        let commit_window = 100u64;
        let reveal_window = 200u64;

        // Register 1000 commits.
        let mut payloads = Vec::with_capacity(1000);
        for i in 0..1000u64 {
            let data =
                serde_json::json!({"temperature": 20.0 + (i as f64) * 0.01, "humidity": 0.5});
            let nonce = format!("nonce-{i}");
            let station_id = format!("station-{i}");
            let timestamp = 1_700_000_000i64 + i as i64;
            let commit_id = Transaction::compute_commit_id(
                &data,
                &nonce,
                &station_id,
                timestamp,
                "contribution",
            );
            let commit = CommitPayload {
                commit_id: commit_id.clone(),
                station_id: station_id.clone(),
                timestamp,
                contribution_type: "contribution".to_string(),
                auth_pubkey: None,
                auth_signature: None,
            };
            state.register_commit(&commit, block_height).unwrap();
            payloads.push(RevealPayload {
                commit_id,
                station_id,
                timestamp,
                contribution_type: "contribution".to_string(),
                data,
                nonce,
                data_cid: None,
                data_hash: None,
                storage_provider: None,
                provenance: None,
                manifest: None,
                verification_receipt: None,
                compute_receipt: None,
                quality_tier: None,
            });
        }
        assert_eq!(state.pending_commits.len(), 1000);

        // Apply 100 reveals and time the operation.
        let start = std::time::Instant::now();
        for payload in &payloads[..100] {
            state
                .apply_reveal(payload, block_height + 1)
                .expect("reveal should succeed");
        }
        // Single batch prune (the fix).
        state.prune_expired(block_height + commit_window + reveal_window);
        let elapsed = start.elapsed();

        assert_eq!(state.applied_reveals.len(), 100);
        assert!(
            elapsed.as_millis() < 100,
            "1000 commits + 100 reveals took {elapsed:?}, expected <100ms"
        );
    }
}
