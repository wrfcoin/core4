//! Actor-class policy model feeding equal-liability sanctions.
//!
//! This layer tracks actor registration, attestations, and class transitions.
//! It is intentionally separated from sanction outcomes: `ActorClass` can be
//! recorded for policy/audit purposes, but sanction outcomes remain class-neutral.

use super::sanctions::{ActorClass, OffenseCode, SanctionMatrix, SanctionOutcome, SeverityTier};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

const MAX_ACTOR_ID_LEN: usize = 128;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActorClassPolicyConfig {
    pub min_attestations: u16,
    pub allow_self_attestation: bool,
    pub registration_enabled: bool,
}

impl Default for ActorClassPolicyConfig {
    fn default() -> Self {
        Self {
            min_attestations: 1,
            allow_self_attestation: false,
            registration_enabled: true,
        }
    }
}

impl ActorClassPolicyConfig {
    pub fn validate(&self) -> Result<(), ActorClassPolicyError> {
        if self.min_attestations == 0 {
            return Err(ActorClassPolicyError::InvalidConfig(
                "SP_ACTOR_POLICY_INVALID_CONFIG: min_attestations must be >= 1".to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActorRegistrationRecord {
    pub actor_id: String,
    pub actor_class: ActorClass,
    pub registered_at_block: u64,
    pub last_transition_block: u64,
    pub attesters: BTreeSet<String>,
    pub active: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_evidence_hash: Option<String>,
}

impl ActorRegistrationRecord {
    pub fn is_attested(&self, min_attestations: u16) -> bool {
        self.attesters.len() >= min_attestations as usize
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ActorClassPolicyTransition {
    Register {
        actor_id: String,
        actor_class: ActorClass,
        evidence_hash: Option<String>,
    },
    Attest {
        actor_id: String,
        attester_id: String,
        evidence_hash: Option<String>,
    },
    Reclassify {
        actor_id: String,
        new_class: ActorClass,
        evidence_hash: Option<String>,
    },
    Deactivate {
        actor_id: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActorClassPolicy {
    version: u32,
    config: ActorClassPolicyConfig,
    registrations: BTreeMap<String, ActorRegistrationRecord>,
}

impl Default for ActorClassPolicy {
    fn default() -> Self {
        Self {
            version: 1,
            config: ActorClassPolicyConfig::default(),
            registrations: BTreeMap::new(),
        }
    }
}

impl ActorClassPolicy {
    pub fn version(&self) -> u32 {
        self.version
    }

    pub fn config(&self) -> &ActorClassPolicyConfig {
        &self.config
    }

    pub fn registrations(&self) -> &BTreeMap<String, ActorRegistrationRecord> {
        &self.registrations
    }

    pub fn update_config(
        &mut self,
        config: ActorClassPolicyConfig,
    ) -> Result<(), ActorClassPolicyError> {
        config.validate()?;
        self.config = config;
        Ok(())
    }

    pub fn apply_transition(
        &mut self,
        transition: ActorClassPolicyTransition,
        block_height: u64,
    ) -> Result<(), ActorClassPolicyError> {
        self.config.validate()?;

        match transition {
            ActorClassPolicyTransition::Register {
                actor_id,
                actor_class,
                evidence_hash,
            } => self.register(actor_id, actor_class, evidence_hash, block_height),
            ActorClassPolicyTransition::Attest {
                actor_id,
                attester_id,
                evidence_hash,
            } => self.attest(actor_id, attester_id, evidence_hash, block_height),
            ActorClassPolicyTransition::Reclassify {
                actor_id,
                new_class,
                evidence_hash,
            } => self.reclassify(actor_id, new_class, evidence_hash, block_height),
            ActorClassPolicyTransition::Deactivate { actor_id } => self.deactivate(actor_id),
        }
    }

    pub fn actor_class_for(&self, actor_id: &str) -> Option<ActorClass> {
        let record = self.registrations.get(actor_id)?;
        if !record.active || !record.is_attested(self.config.min_attestations) {
            return None;
        }
        Some(record.actor_class)
    }

    pub fn evaluate_sanction<'a>(
        &self,
        matrix: &'a SanctionMatrix,
        actor_id: &str,
        offense: OffenseCode,
        severity: SeverityTier,
    ) -> Option<&'a SanctionOutcome> {
        let class = self.actor_class_for(actor_id).unwrap_or(ActorClass::Human);
        matrix.lookup(offense, severity, class)
    }

    pub fn assert_equal_liability(
        &self,
        matrix: &SanctionMatrix,
    ) -> Result<(), ActorClassPolicyError> {
        for offense in OffenseCode::all() {
            for severity in SeverityTier::all() {
                let baseline = matrix
                    .lookup(*offense, *severity, ActorClass::Human)
                    .ok_or_else(|| {
                        ActorClassPolicyError::IncompleteMatrix(format!(
                            "SP_ACTOR_POLICY_INCOMPLETE_MATRIX: missing human baseline for ({offense:?}, {severity:?})"
                        ))
                    })?;
                for actor_class in ActorClass::all() {
                    let candidate = matrix.lookup(*offense, *severity, *actor_class).ok_or_else(|| {
                        ActorClassPolicyError::IncompleteMatrix(format!(
                            "SP_ACTOR_POLICY_INCOMPLETE_MATRIX: missing entry for ({offense:?}, {severity:?}, {actor_class:?})"
                        ))
                    })?;
                    if candidate != baseline {
                        return Err(ActorClassPolicyError::EqualLiabilityViolation(format!(
                            "SP_ACTOR_POLICY_EQUAL_LIABILITY_VIOLATION: mismatch at ({offense:?}, {severity:?}, {actor_class:?})"
                        )));
                    }
                }
            }
        }
        Ok(())
    }

    pub fn content_hash_hex(&self) -> Result<String, ActorClassPolicyError> {
        let payload = serde_json::to_vec(self).map_err(|e| {
            ActorClassPolicyError::Serialization(format!(
                "SP_ACTOR_POLICY_SERIALIZATION_ERROR: {e}"
            ))
        })?;
        Ok(blake3::hash(&payload).to_hex().to_string())
    }

    fn register(
        &mut self,
        actor_id: String,
        actor_class: ActorClass,
        evidence_hash: Option<String>,
        block_height: u64,
    ) -> Result<(), ActorClassPolicyError> {
        if !self.config.registration_enabled {
            return Err(ActorClassPolicyError::RegistrationDisabled(
                "SP_ACTOR_POLICY_REGISTRATION_DISABLED: registrations are currently disabled"
                    .to_string(),
            ));
        }
        validate_actor_id(&actor_id)?;
        if self.registrations.contains_key(&actor_id) {
            return Err(ActorClassPolicyError::AlreadyRegistered(format!(
                "SP_ACTOR_POLICY_ALREADY_REGISTERED: actor '{actor_id}' is already registered"
            )));
        }
        let record = ActorRegistrationRecord {
            actor_id: actor_id.clone(),
            actor_class,
            registered_at_block: block_height,
            last_transition_block: block_height,
            attesters: BTreeSet::new(),
            active: true,
            latest_evidence_hash: evidence_hash,
        };
        self.registrations.insert(actor_id, record);
        Ok(())
    }

    fn attest(
        &mut self,
        actor_id: String,
        attester_id: String,
        evidence_hash: Option<String>,
        block_height: u64,
    ) -> Result<(), ActorClassPolicyError> {
        validate_actor_id(&actor_id)?;
        validate_actor_id(&attester_id)?;

        let record = self.registrations.get_mut(&actor_id).ok_or_else(|| {
            ActorClassPolicyError::NotRegistered(format!(
                "SP_ACTOR_POLICY_NOT_REGISTERED: actor '{actor_id}' is not registered"
            ))
        })?;
        if !record.active {
            return Err(ActorClassPolicyError::InactiveActor(format!(
                "SP_ACTOR_POLICY_INACTIVE_ACTOR: actor '{actor_id}' is inactive"
            )));
        }
        if !self.config.allow_self_attestation && actor_id == attester_id {
            return Err(ActorClassPolicyError::SelfAttestationDisabled(
                "SP_ACTOR_POLICY_SELF_ATTESTATION_DISABLED: actor cannot self-attest".to_string(),
            ));
        }
        if !record.attesters.insert(attester_id) {
            return Err(ActorClassPolicyError::DuplicateAttestation(format!(
                "SP_ACTOR_POLICY_DUPLICATE_ATTESTATION: duplicate attester for actor '{actor_id}'"
            )));
        }
        record.last_transition_block = block_height;
        if evidence_hash.is_some() {
            record.latest_evidence_hash = evidence_hash;
        }
        Ok(())
    }

    fn reclassify(
        &mut self,
        actor_id: String,
        new_class: ActorClass,
        evidence_hash: Option<String>,
        block_height: u64,
    ) -> Result<(), ActorClassPolicyError> {
        validate_actor_id(&actor_id)?;

        let record = self.registrations.get_mut(&actor_id).ok_or_else(|| {
            ActorClassPolicyError::NotRegistered(format!(
                "SP_ACTOR_POLICY_NOT_REGISTERED: actor '{actor_id}' is not registered"
            ))
        })?;
        if !record.active {
            return Err(ActorClassPolicyError::InactiveActor(format!(
                "SP_ACTOR_POLICY_INACTIVE_ACTOR: actor '{actor_id}' is inactive"
            )));
        }
        if !record.is_attested(self.config.min_attestations) {
            return Err(ActorClassPolicyError::InsufficientAttestations(format!(
                "SP_ACTOR_POLICY_INSUFFICIENT_ATTESTATIONS: actor '{actor_id}' needs at least {} attestations",
                self.config.min_attestations
            )));
        }
        record.actor_class = new_class;
        record.last_transition_block = block_height;
        if evidence_hash.is_some() {
            record.latest_evidence_hash = evidence_hash;
        }
        Ok(())
    }

    fn deactivate(&mut self, actor_id: String) -> Result<(), ActorClassPolicyError> {
        validate_actor_id(&actor_id)?;
        let record = self.registrations.get_mut(&actor_id).ok_or_else(|| {
            ActorClassPolicyError::NotRegistered(format!(
                "SP_ACTOR_POLICY_NOT_REGISTERED: actor '{actor_id}' is not registered"
            ))
        })?;
        record.active = false;
        Ok(())
    }
}

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum ActorClassPolicyError {
    #[error("{0}")]
    InvalidConfig(String),
    #[error("{0}")]
    InvalidActorId(String),
    #[error("{0}")]
    RegistrationDisabled(String),
    #[error("{0}")]
    AlreadyRegistered(String),
    #[error("{0}")]
    NotRegistered(String),
    #[error("{0}")]
    InactiveActor(String),
    #[error("{0}")]
    DuplicateAttestation(String),
    #[error("{0}")]
    SelfAttestationDisabled(String),
    #[error("{0}")]
    InsufficientAttestations(String),
    #[error("{0}")]
    EqualLiabilityViolation(String),
    #[error("{0}")]
    IncompleteMatrix(String),
    #[error("{0}")]
    Serialization(String),
}

fn validate_actor_id(actor_id: &str) -> Result<(), ActorClassPolicyError> {
    if actor_id.trim().is_empty() {
        return Err(ActorClassPolicyError::InvalidActorId(
            "SP_ACTOR_POLICY_INVALID_ACTOR_ID: actor_id cannot be empty".to_string(),
        ));
    }
    if actor_id.trim() != actor_id {
        return Err(ActorClassPolicyError::InvalidActorId(
            "SP_ACTOR_POLICY_INVALID_ACTOR_ID: actor_id cannot have leading/trailing whitespace"
                .to_string(),
        ));
    }
    if actor_id.len() > MAX_ACTOR_ID_LEN {
        return Err(ActorClassPolicyError::InvalidActorId(format!(
            "SP_ACTOR_POLICY_INVALID_ACTOR_ID: actor_id exceeds max length {MAX_ACTOR_ID_LEN}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registration_attestation_reclassify_flow_succeeds() {
        let mut policy = ActorClassPolicy::default();
        policy
            .apply_transition(
                ActorClassPolicyTransition::Register {
                    actor_id: "actor-1".to_string(),
                    actor_class: ActorClass::Human,
                    evidence_hash: Some("abc".to_string()),
                },
                10,
            )
            .expect("register should pass");

        assert_eq!(policy.actor_class_for("actor-1"), None);

        policy
            .apply_transition(
                ActorClassPolicyTransition::Attest {
                    actor_id: "actor-1".to_string(),
                    attester_id: "auditor-1".to_string(),
                    evidence_hash: None,
                },
                11,
            )
            .expect("attestation should pass");

        assert_eq!(policy.actor_class_for("actor-1"), Some(ActorClass::Human));

        policy
            .apply_transition(
                ActorClassPolicyTransition::Reclassify {
                    actor_id: "actor-1".to_string(),
                    new_class: ActorClass::AiAgent,
                    evidence_hash: Some("def".to_string()),
                },
                12,
            )
            .expect("reclassify should pass");

        assert_eq!(policy.actor_class_for("actor-1"), Some(ActorClass::AiAgent));
    }

    #[test]
    fn reclassify_requires_attestation_threshold() {
        let mut policy = ActorClassPolicy::default();
        policy
            .update_config(ActorClassPolicyConfig {
                min_attestations: 2,
                ..ActorClassPolicyConfig::default()
            })
            .expect("config update should pass");
        policy
            .apply_transition(
                ActorClassPolicyTransition::Register {
                    actor_id: "actor-2".to_string(),
                    actor_class: ActorClass::Robot,
                    evidence_hash: None,
                },
                1,
            )
            .expect("register should pass");
        policy
            .apply_transition(
                ActorClassPolicyTransition::Attest {
                    actor_id: "actor-2".to_string(),
                    attester_id: "auditor-a".to_string(),
                    evidence_hash: None,
                },
                2,
            )
            .expect("first attestation should pass");

        let err = policy
            .apply_transition(
                ActorClassPolicyTransition::Reclassify {
                    actor_id: "actor-2".to_string(),
                    new_class: ActorClass::Human,
                    evidence_hash: None,
                },
                3,
            )
            .expect_err("reclassify should fail with insufficient attestations");
        assert!(
            err.to_string()
                .contains("SP_ACTOR_POLICY_INSUFFICIENT_ATTESTATIONS")
        );
    }

    #[test]
    fn equal_liability_invariant_holds_for_v1_matrix() {
        let policy = ActorClassPolicy::default();
        let matrix = SanctionMatrix::v1();
        policy
            .assert_equal_liability(&matrix)
            .expect("v1 matrix should satisfy equal liability");
    }

    #[test]
    fn policy_content_hash_is_deterministic() {
        let mut a = ActorClassPolicy::default();
        let mut b = ActorClassPolicy::default();

        let transition = ActorClassPolicyTransition::Register {
            actor_id: "actor-3".to_string(),
            actor_class: ActorClass::Human,
            evidence_hash: Some("hash".to_string()),
        };
        a.apply_transition(transition.clone(), 9).unwrap();
        b.apply_transition(transition, 9).unwrap();

        assert_eq!(
            a.content_hash_hex().unwrap(),
            b.content_hash_hex().unwrap(),
            "identical policy state must hash identically"
        );
    }
}
