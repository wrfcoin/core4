//! Governance module for Wrfcoin
//!
//! This module provides mechanisms for protocol-level governance and decision-making.

pub mod actor_policy;
pub mod implementation;
pub mod policy_engine;
pub mod proposal;
pub mod sanctions;
pub mod faucet_sink_pop;
pub mod schedule_extension;
pub mod upgrade_notice;
pub mod voting;

pub use actor_policy::{
    ActorClassPolicy, ActorClassPolicyConfig, ActorClassPolicyError, ActorClassPolicyTransition,
    ActorRegistrationRecord,
};
pub use implementation::{AutomaticImplementation, GovernanceImplementor};
pub use policy_engine::{CompiledPolicy, PolicyCompileError, PolicyCompiler};
pub use proposal::{Proposal, ProposalStatus, ProposalType};
pub use sanctions::{
    ActorClass, OffenseCategory, OffenseCode, SanctionMatrix, SanctionOutcome, SeverityTier,
};
pub use voting::{Vote, VotingPower, VotingSystem};

use crate::blockchain::state::TrustWeightingPolicy;
use crate::common::constants::{GOVERNANCE_DEFAULT_DEPOSIT, GOVERNANCE_DEFAULT_QUORUM_PERCENT};
use crate::weather::validation::{DataQualityBand, DataQualityPolicy};
use anyhow::Result;
use chrono::{DateTime, Utc};
use log::{error, info};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Parameter targets controlled by DAO governance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GovernedParameter {
    OracleSet,
    OracleQuorum,
    StakeRequired,
    EmissionRate,
    CommitWindow,
    RevealWindow,
    PopUsefulWorkActivationHeight,
    QualityHighThreshold,
    QualityAcceptanceThreshold,
    QualityWarningThreshold,
    QualityBorderlineScoringMultiplier,
    QualityBorderlineRewardMultiplier,
    QualityWarningScoringMultiplier,
    QualityWarningRewardMultiplier,
    QualityLowScoringMultiplier,
    QualityLowRewardMultiplier,
    QualityLowSlashStreak,
    QualityLowSlashAmount,
    TrustUnverifiedWeight,
    TrustVerifiedStationWeight,
    TrustTrustedProviderWeight,
    TaskAssignerEnabled,
    TaskAssignerUnderservedBonusMultiplier,
    TaskAssignerRedundancyPenaltyMultiplier,
    ScarcityDataDesertMultiplier,
    ScarcityProximityMultiplier,
    MultidomainSoilEnabled,
    MultidomainOceanEnabled,
    MultidomainHydrologyEnabled,
    MultidomainSchemaVersionMin,
    MultidomainSoilWeight,
    MultidomainOceanWeight,
    MultidomainHydrologyWeight,
    ActorClassMinAttestations,
    ActorClassSelfAttestationEnabled,
    ActorClassRegistrationEnabled,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum GovernancePreviewError {
    #[error(
        "SP_GOV_PREVIEW_NON_QUALITY_TARGET: target must be one of the quality.* governance parameters"
    )]
    NonQualityTarget,
    #[error(
        "SP_GOV_PREVIEW_NON_TRUST_TARGET: target must be one of the trust.* governance parameters"
    )]
    NonTrustTarget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QualityGovernedParameter {
    HighThreshold,
    AcceptanceThreshold,
    WarningThreshold,
    BorderlineScoringMultiplier,
    BorderlineRewardMultiplier,
    WarningScoringMultiplier,
    WarningRewardMultiplier,
    LowScoringMultiplier,
    LowRewardMultiplier,
    LowSlashStreak,
    LowSlashAmount,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrustGovernedParameter {
    UnverifiedWeight,
    VerifiedStationWeight,
    TrustedProviderWeight,
}

impl TryFrom<GovernedParameter> for QualityGovernedParameter {
    type Error = GovernancePreviewError;

    fn try_from(value: GovernedParameter) -> std::result::Result<Self, Self::Error> {
        match value {
            GovernedParameter::QualityHighThreshold => Ok(Self::HighThreshold),
            GovernedParameter::QualityAcceptanceThreshold => Ok(Self::AcceptanceThreshold),
            GovernedParameter::QualityWarningThreshold => Ok(Self::WarningThreshold),
            GovernedParameter::QualityBorderlineScoringMultiplier => {
                Ok(Self::BorderlineScoringMultiplier)
            },
            GovernedParameter::QualityBorderlineRewardMultiplier => {
                Ok(Self::BorderlineRewardMultiplier)
            },
            GovernedParameter::QualityWarningScoringMultiplier => {
                Ok(Self::WarningScoringMultiplier)
            },
            GovernedParameter::QualityWarningRewardMultiplier => Ok(Self::WarningRewardMultiplier),
            GovernedParameter::QualityLowScoringMultiplier => Ok(Self::LowScoringMultiplier),
            GovernedParameter::QualityLowRewardMultiplier => Ok(Self::LowRewardMultiplier),
            GovernedParameter::QualityLowSlashStreak => Ok(Self::LowSlashStreak),
            GovernedParameter::QualityLowSlashAmount => Ok(Self::LowSlashAmount),
            _ => Err(GovernancePreviewError::NonQualityTarget),
        }
    }
}

impl TryFrom<GovernedParameter> for TrustGovernedParameter {
    type Error = GovernancePreviewError;

    fn try_from(value: GovernedParameter) -> std::result::Result<Self, Self::Error> {
        match value {
            GovernedParameter::TrustUnverifiedWeight => Ok(Self::UnverifiedWeight),
            GovernedParameter::TrustVerifiedStationWeight => Ok(Self::VerifiedStationWeight),
            GovernedParameter::TrustTrustedProviderWeight => Ok(Self::TrustedProviderWeight),
            _ => Err(GovernancePreviewError::NonTrustTarget),
        }
    }
}

/// Structured parameter update proposal payload.
#[derive(Debug, Clone)]
pub struct ParameterUpdateProposal {
    pub target: GovernedParameter,
    pub value: String,
}

impl ParameterUpdateProposal {
    pub fn validate(&self) -> Result<()> {
        fn parse_finite_unit_interval(value: &str, field: &str) -> Result<f64> {
            let parsed = value
                .parse::<f64>()
                .map_err(|_| anyhow::anyhow!("{field} must be a number"))?;
            if !parsed.is_finite() || !(0.0..=1.0).contains(&parsed) {
                return Err(anyhow::anyhow!("{field} must be in [0, 1]"));
            }
            Ok(parsed)
        }
        fn parse_nonnegative_finite(value: &str, field: &str) -> Result<f64> {
            let parsed = value
                .parse::<f64>()
                .map_err(|_| anyhow::anyhow!("{field} must be a number"))?;
            if !parsed.is_finite() || parsed < 0.0 {
                return Err(anyhow::anyhow!("{field} must be finite and >= 0"));
            }
            Ok(parsed)
        }

        match self.target {
            GovernedParameter::OracleSet => {
                if self.value.trim().is_empty() {
                    return Err(anyhow::anyhow!("oracle_set cannot be empty"));
                }
            },
            GovernedParameter::OracleQuorum => {
                let v = self
                    .value
                    .parse::<usize>()
                    .map_err(|_| anyhow::anyhow!("oracle_quorum must be integer"))?;
                if v == 0 {
                    return Err(anyhow::anyhow!("oracle_quorum must be > 0"));
                }
            },
            GovernedParameter::StakeRequired => {
                let parts = self.value.split(':').collect::<Vec<_>>();
                if parts.len() != 2 {
                    return Err(anyhow::anyhow!(
                        "stake_required must use address_hex:amount format"
                    ));
                }
                let _ = parts[1]
                    .parse::<u128>()
                    .map_err(|_| anyhow::anyhow!("stake amount must be integer"))?;
            },
            GovernedParameter::EmissionRate
            | GovernedParameter::CommitWindow
            | GovernedParameter::RevealWindow => {
                let _ = self
                    .value
                    .parse::<u64>()
                    .map_err(|_| anyhow::anyhow!("value must be integer"))?;
            },
            GovernedParameter::PopUsefulWorkActivationHeight => {
                let parsed = self
                    .value
                    .parse::<u64>()
                    .map_err(|_| anyhow::anyhow!("value must be integer"))?;
                if parsed != 0 {
                    return Err(anyhow::anyhow!(
                        "pop.useful_work_activation_height must be 0 in useful-work-only mode"
                    ));
                }
            },
            GovernedParameter::QualityHighThreshold => {
                parse_finite_unit_interval(&self.value, "quality.high_threshold")?;
            },
            GovernedParameter::QualityAcceptanceThreshold => {
                parse_finite_unit_interval(&self.value, "quality.acceptance_threshold")?;
            },
            GovernedParameter::QualityWarningThreshold => {
                parse_finite_unit_interval(&self.value, "quality.warning_threshold")?;
            },
            GovernedParameter::QualityBorderlineScoringMultiplier => {
                parse_finite_unit_interval(&self.value, "quality.borderline_scoring_multiplier")?;
            },
            GovernedParameter::QualityBorderlineRewardMultiplier => {
                parse_finite_unit_interval(&self.value, "quality.borderline_reward_multiplier")?;
            },
            GovernedParameter::QualityWarningScoringMultiplier => {
                parse_finite_unit_interval(&self.value, "quality.warning_scoring_multiplier")?;
            },
            GovernedParameter::QualityWarningRewardMultiplier => {
                parse_finite_unit_interval(&self.value, "quality.warning_reward_multiplier")?;
            },
            GovernedParameter::QualityLowScoringMultiplier => {
                parse_finite_unit_interval(&self.value, "quality.low_scoring_multiplier")?;
            },
            GovernedParameter::QualityLowRewardMultiplier => {
                parse_finite_unit_interval(&self.value, "quality.low_reward_multiplier")?;
            },
            GovernedParameter::QualityLowSlashStreak => {
                let streak = self
                    .value
                    .parse::<u32>()
                    .map_err(|_| anyhow::anyhow!("quality.low_slash_streak must be integer"))?;
                if streak == 0 {
                    return Err(anyhow::anyhow!("quality.low_slash_streak must be >= 1"));
                }
            },
            GovernedParameter::QualityLowSlashAmount => {
                let amount = self
                    .value
                    .parse::<u128>()
                    .map_err(|_| anyhow::anyhow!("quality.low_slash_amount must be integer"))?;
                if amount == 0 {
                    return Err(anyhow::anyhow!("quality.low_slash_amount must be >= 1"));
                }
            },
            GovernedParameter::TrustUnverifiedWeight => {
                parse_finite_unit_interval(&self.value, "trust.unverified_weight")?;
            },
            GovernedParameter::TrustVerifiedStationWeight => {
                parse_finite_unit_interval(&self.value, "trust.verified_station_weight")?;
            },
            GovernedParameter::TrustTrustedProviderWeight => {
                parse_finite_unit_interval(&self.value, "trust.trusted_provider_weight")?;
            },
            GovernedParameter::TaskAssignerEnabled
            | GovernedParameter::MultidomainSoilEnabled
            | GovernedParameter::MultidomainOceanEnabled
            | GovernedParameter::MultidomainHydrologyEnabled => {
                let _ = self
                    .value
                    .parse::<bool>()
                    .map_err(|_| anyhow::anyhow!("value must be boolean"))?;
            },
            GovernedParameter::TaskAssignerUnderservedBonusMultiplier => {
                let value = parse_nonnegative_finite(
                    &self.value,
                    "task_assigner.underserved_bonus_multiplier",
                )?;
                if value < 1.0 {
                    return Err(anyhow::anyhow!(
                        "task_assigner.underserved_bonus_multiplier must be >= 1.0"
                    ));
                }
            },
            GovernedParameter::TaskAssignerRedundancyPenaltyMultiplier => {
                parse_finite_unit_interval(
                    &self.value,
                    "task_assigner.redundancy_penalty_multiplier",
                )?;
            },
            GovernedParameter::ScarcityDataDesertMultiplier => {
                parse_nonnegative_finite(&self.value, "scarcity.data_desert_multiplier")?;
            },
            GovernedParameter::ScarcityProximityMultiplier => {
                parse_nonnegative_finite(&self.value, "scarcity.proximity_multiplier")?;
            },
            GovernedParameter::MultidomainSchemaVersionMin => {
                let _ = self.value.parse::<u32>().map_err(|_| {
                    anyhow::anyhow!("multidomain.schema_version_min must be integer")
                })?;
            },
            GovernedParameter::MultidomainSoilWeight => {
                parse_nonnegative_finite(&self.value, "multidomain.soil_weight")?;
            },
            GovernedParameter::MultidomainOceanWeight => {
                parse_nonnegative_finite(&self.value, "multidomain.ocean_weight")?;
            },
            GovernedParameter::MultidomainHydrologyWeight => {
                parse_nonnegative_finite(&self.value, "multidomain.hydrology_weight")?;
            },
            GovernedParameter::ActorClassMinAttestations => {
                let attestations = self
                    .value
                    .parse::<u16>()
                    .map_err(|_| anyhow::anyhow!("actor_class.min_attestations must be integer"))?;
                if attestations == 0 {
                    return Err(anyhow::anyhow!("actor_class.min_attestations must be >= 1"));
                }
            },
            GovernedParameter::ActorClassSelfAttestationEnabled
            | GovernedParameter::ActorClassRegistrationEnabled => {
                let _ = self.value.parse::<bool>().map_err(|_| {
                    anyhow::anyhow!("actor_class policy toggles must be true|false")
                })?;
            },
        }
        Ok(())
    }

    pub fn to_parameter_change(&self) -> (String, String) {
        let key = match self.target {
            GovernedParameter::OracleSet => "oracle_set",
            GovernedParameter::OracleQuorum => "oracle_quorum",
            GovernedParameter::StakeRequired => "stake_required",
            GovernedParameter::EmissionRate => "emission_rate",
            GovernedParameter::CommitWindow => "commit_window",
            GovernedParameter::RevealWindow => "reveal_window",
            GovernedParameter::PopUsefulWorkActivationHeight => "pop.useful_work_activation_height",
            GovernedParameter::QualityHighThreshold => "quality.high_threshold",
            GovernedParameter::QualityAcceptanceThreshold => "quality.acceptance_threshold",
            GovernedParameter::QualityWarningThreshold => "quality.warning_threshold",
            GovernedParameter::QualityBorderlineScoringMultiplier => {
                "quality.borderline_scoring_multiplier"
            },
            GovernedParameter::QualityBorderlineRewardMultiplier => {
                "quality.borderline_reward_multiplier"
            },
            GovernedParameter::QualityWarningScoringMultiplier => {
                "quality.warning_scoring_multiplier"
            },
            GovernedParameter::QualityWarningRewardMultiplier => {
                "quality.warning_reward_multiplier"
            },
            GovernedParameter::QualityLowScoringMultiplier => "quality.low_scoring_multiplier",
            GovernedParameter::QualityLowRewardMultiplier => "quality.low_reward_multiplier",
            GovernedParameter::QualityLowSlashStreak => "quality.low_slash_streak",
            GovernedParameter::QualityLowSlashAmount => "quality.low_slash_amount",
            GovernedParameter::TrustUnverifiedWeight => "trust.unverified_weight",
            GovernedParameter::TrustVerifiedStationWeight => "trust.verified_station_weight",
            GovernedParameter::TrustTrustedProviderWeight => "trust.trusted_provider_weight",
            GovernedParameter::TaskAssignerEnabled => "task_assigner.enabled",
            GovernedParameter::TaskAssignerUnderservedBonusMultiplier => {
                "task_assigner.underserved_bonus_multiplier"
            },
            GovernedParameter::TaskAssignerRedundancyPenaltyMultiplier => {
                "task_assigner.redundancy_penalty_multiplier"
            },
            GovernedParameter::ScarcityDataDesertMultiplier => "scarcity.data_desert_multiplier",
            GovernedParameter::ScarcityProximityMultiplier => "scarcity.proximity_multiplier",
            GovernedParameter::MultidomainSoilEnabled => "multidomain.soil_enabled",
            GovernedParameter::MultidomainOceanEnabled => "multidomain.ocean_enabled",
            GovernedParameter::MultidomainHydrologyEnabled => "multidomain.hydrology_enabled",
            GovernedParameter::MultidomainSchemaVersionMin => "multidomain.schema_version_min",
            GovernedParameter::MultidomainSoilWeight => "multidomain.soil_weight",
            GovernedParameter::MultidomainOceanWeight => "multidomain.ocean_weight",
            GovernedParameter::MultidomainHydrologyWeight => "multidomain.hydrology_weight",
            GovernedParameter::ActorClassMinAttestations => "actor_class.min_attestations",
            GovernedParameter::ActorClassSelfAttestationEnabled => {
                "actor_class.self_attestation_enabled"
            },
            GovernedParameter::ActorClassRegistrationEnabled => "actor_class.registration_enabled",
        };
        (key.to_string(), self.value.clone())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TrustWeightBoundsPreview {
    pub min: f64,
    pub max: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TrustWeightOrderingHint {
    pub rule: String,
    pub satisfied: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TrustClassMultiplierPreview {
    pub contributor_class: String,
    pub trust_weight: f64,
    pub relative_to_trusted_provider: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TrustProposalPreview {
    pub parameter: String,
    pub value: String,
    pub current_policy: TrustWeightingPolicy,
    pub resulting_policy: TrustWeightingPolicy,
    pub bounds: TrustWeightBoundsPreview,
    pub ordering_hints: Vec<TrustWeightOrderingHint>,
    pub class_multipliers: Vec<TrustClassMultiplierPreview>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QualityBandPreview {
    pub band: DataQualityBand,
    pub min_inclusive: f64,
    pub max_exclusive: Option<f64>,
    pub scoring_multiplier: f64,
    pub reward_multiplier: f64,
    pub warning_only: bool,
    pub trigger_challenge: bool,
    pub counts_toward_low_quality_streak: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QualityProposalPreview {
    pub parameter: String,
    pub value: String,
    pub resulting_policy: DataQualityPolicy,
    pub band_outcomes: Vec<QualityBandPreview>,
}

impl QualityProposalPreview {
    fn from_policy(parameter: String, value: String, policy: DataQualityPolicy) -> Self {
        let band_outcomes = vec![
            QualityBandPreview {
                band: DataQualityBand::High,
                min_inclusive: policy.high_quality_threshold,
                max_exclusive: None,
                scoring_multiplier: 1.0,
                reward_multiplier: 1.0,
                warning_only: false,
                trigger_challenge: false,
                counts_toward_low_quality_streak: false,
            },
            QualityBandPreview {
                band: DataQualityBand::Borderline,
                min_inclusive: policy.min_acceptance_threshold,
                max_exclusive: Some(policy.high_quality_threshold),
                scoring_multiplier: policy.borderline_scoring_multiplier,
                reward_multiplier: policy.borderline_reward_multiplier,
                warning_only: false,
                trigger_challenge: false,
                counts_toward_low_quality_streak: false,
            },
            QualityBandPreview {
                band: DataQualityBand::WarningOnly,
                min_inclusive: policy.warning_only_threshold,
                max_exclusive: Some(policy.min_acceptance_threshold),
                scoring_multiplier: policy.warning_only_scoring_multiplier,
                reward_multiplier: policy.warning_only_reward_multiplier,
                warning_only: true,
                trigger_challenge: true,
                counts_toward_low_quality_streak: false,
            },
            QualityBandPreview {
                band: DataQualityBand::Low,
                min_inclusive: 0.0,
                max_exclusive: Some(policy.warning_only_threshold),
                scoring_multiplier: policy.low_quality_scoring_multiplier,
                reward_multiplier: policy.low_quality_reward_multiplier,
                warning_only: false,
                trigger_challenge: true,
                counts_toward_low_quality_streak: true,
            },
        ];

        Self {
            parameter,
            value,
            resulting_policy: policy,
            band_outcomes,
        }
    }
}

pub fn build_quality_parameter_preview(
    current_policy: &DataQualityPolicy,
    update: ParameterUpdateProposal,
) -> Result<QualityProposalPreview> {
    let quality_target = QualityGovernedParameter::try_from(update.target)?;

    update.validate()?;
    let mut next_policy = current_policy.clone();

    match quality_target {
        QualityGovernedParameter::HighThreshold => {
            next_policy.high_quality_threshold = update.value.parse::<f64>()?;
        },
        QualityGovernedParameter::AcceptanceThreshold => {
            next_policy.min_acceptance_threshold = update.value.parse::<f64>()?;
        },
        QualityGovernedParameter::WarningThreshold => {
            next_policy.warning_only_threshold = update.value.parse::<f64>()?;
        },
        QualityGovernedParameter::BorderlineScoringMultiplier => {
            next_policy.borderline_scoring_multiplier = update.value.parse::<f64>()?;
        },
        QualityGovernedParameter::BorderlineRewardMultiplier => {
            next_policy.borderline_reward_multiplier = update.value.parse::<f64>()?;
        },
        QualityGovernedParameter::WarningScoringMultiplier => {
            next_policy.warning_only_scoring_multiplier = update.value.parse::<f64>()?;
        },
        QualityGovernedParameter::WarningRewardMultiplier => {
            next_policy.warning_only_reward_multiplier = update.value.parse::<f64>()?;
        },
        QualityGovernedParameter::LowScoringMultiplier => {
            next_policy.low_quality_scoring_multiplier = update.value.parse::<f64>()?;
        },
        QualityGovernedParameter::LowRewardMultiplier => {
            next_policy.low_quality_reward_multiplier = update.value.parse::<f64>()?;
        },
        QualityGovernedParameter::LowSlashStreak => {
            next_policy.low_quality_slash_streak = update.value.parse::<u32>()?;
        },
        QualityGovernedParameter::LowSlashAmount => {
            next_policy.low_quality_slash_amount = update.value.parse::<u128>()?;
        },
    }

    next_policy
        .validate()
        .map_err(|e| anyhow::anyhow!("invalid quality policy update: {e}"))?;

    let (parameter, value) = update.to_parameter_change();
    Ok(QualityProposalPreview::from_policy(
        parameter,
        value,
        next_policy,
    ))
}

fn trust_ordering_hints(policy: &TrustWeightingPolicy) -> Vec<TrustWeightOrderingHint> {
    vec![
        TrustWeightOrderingHint {
            rule: "trust.verified_station_weight >= trust.unverified_weight".to_string(),
            satisfied: policy.verified_station_weight >= policy.unverified_weight,
        },
        TrustWeightOrderingHint {
            rule: "trust.trusted_provider_weight >= trust.verified_station_weight".to_string(),
            satisfied: policy.trusted_provider_weight >= policy.verified_station_weight,
        },
    ]
}

fn build_trust_class_multiplier_preview(
    policy: &TrustWeightingPolicy,
) -> Vec<TrustClassMultiplierPreview> {
    let trusted = policy.trusted_provider_weight;
    let relative = |value: f64| {
        if trusted <= f64::EPSILON {
            0.0
        } else {
            value / trusted
        }
    };
    vec![
        TrustClassMultiplierPreview {
            contributor_class: "unverified".to_string(),
            trust_weight: policy.unverified_weight,
            relative_to_trusted_provider: relative(policy.unverified_weight),
        },
        TrustClassMultiplierPreview {
            contributor_class: "verified_station".to_string(),
            trust_weight: policy.verified_station_weight,
            relative_to_trusted_provider: relative(policy.verified_station_weight),
        },
        TrustClassMultiplierPreview {
            contributor_class: "trusted_provider".to_string(),
            trust_weight: policy.trusted_provider_weight,
            relative_to_trusted_provider: relative(policy.trusted_provider_weight),
        },
    ]
}

fn validate_trust_weighting_policy(policy: &TrustWeightingPolicy) -> Result<()> {
    fn validate_weight(name: &str, value: f64) -> Result<()> {
        if !value.is_finite() || !(0.0..=1.0).contains(&value) {
            return Err(anyhow::anyhow!("{name} must be a finite value in [0, 1]"));
        }
        Ok(())
    }

    validate_weight("trust.unverified_weight", policy.unverified_weight)?;
    validate_weight(
        "trust.verified_station_weight",
        policy.verified_station_weight,
    )?;
    validate_weight(
        "trust.trusted_provider_weight",
        policy.trusted_provider_weight,
    )?;

    if policy.verified_station_weight < policy.unverified_weight {
        return Err(anyhow::anyhow!(
            "trust.verified_station_weight must be >= trust.unverified_weight"
        ));
    }
    if policy.trusted_provider_weight < policy.verified_station_weight {
        return Err(anyhow::anyhow!(
            "trust.trusted_provider_weight must be >= trust.verified_station_weight"
        ));
    }
    Ok(())
}

pub fn build_trust_weight_parameter_preview(
    current_policy: &TrustWeightingPolicy,
    update: ParameterUpdateProposal,
) -> Result<TrustProposalPreview> {
    let trust_target = TrustGovernedParameter::try_from(update.target)?;

    update.validate()?;
    let mut next_policy = current_policy.clone();

    match trust_target {
        TrustGovernedParameter::UnverifiedWeight => {
            next_policy.unverified_weight = update.value.parse::<f64>()?;
        },
        TrustGovernedParameter::VerifiedStationWeight => {
            next_policy.verified_station_weight = update.value.parse::<f64>()?;
        },
        TrustGovernedParameter::TrustedProviderWeight => {
            next_policy.trusted_provider_weight = update.value.parse::<f64>()?;
        },
    }

    validate_trust_weighting_policy(&next_policy)
        .map_err(|e| anyhow::anyhow!("invalid trust weighting update: {e}"))?;

    let (parameter, value) = update.to_parameter_change();
    Ok(TrustProposalPreview {
        parameter,
        value,
        current_policy: current_policy.clone(),
        resulting_policy: next_policy.clone(),
        bounds: TrustWeightBoundsPreview { min: 0.0, max: 1.0 },
        ordering_hints: trust_ordering_hints(&next_policy),
        class_multipliers: build_trust_class_multiplier_preview(&next_policy),
    })
}

/// Governance parameters
#[derive(Debug, Clone)]
pub struct GovernanceParams {
    /// Minimum voting period in seconds
    pub min_voting_period: u64,

    /// Maximum voting period in seconds
    pub max_voting_period: u64,

    /// Minimum quorum percentage (0-100)
    pub min_quorum: u8,

    /// Minimum approval percentage (0-100)
    pub min_approval: u8,

    /// Minimum proposal deposit
    pub min_deposit: u64,
}

impl Default for GovernanceParams {
    fn default() -> Self {
        Self {
            min_voting_period: 86400,  // 1 day
            max_voting_period: 604800, // 1 week
            min_quorum: GOVERNANCE_DEFAULT_QUORUM_PERCENT as u8,
            min_approval: 51, // 51%
            min_deposit: GOVERNANCE_DEFAULT_DEPOSIT as u64,
        }
    }
}

/// Main governance system
pub struct Governance {
    /// Governance parameters
    params: GovernanceParams,

    /// Active proposals
    proposals: HashMap<String, Proposal>,

    /// Voting system implementation
    voting_system: Arc<RwLock<Box<dyn VotingSystem>>>,

    /// Governance implementation
    implementation: Arc<RwLock<Box<dyn GovernanceImplementor>>>,

    /// Total voting power in the system
    total_voting_power: VotingPower,
}

impl Governance {
    /// Create a new governance system
    pub fn new(
        params: GovernanceParams,
        voting_system: Box<dyn VotingSystem>,
        implementation: Box<dyn GovernanceImplementor>,
    ) -> Self {
        Self {
            params,
            proposals: HashMap::new(),
            voting_system: Arc::new(RwLock::new(voting_system)),
            implementation: Arc::new(RwLock::new(implementation)),
            total_voting_power: VotingPower::default(),
        }
    }

    /// Create a new proposal.
    ///
    /// `current_time` must be supplied by the caller (typically `block.timestamp`) so that
    /// proposal deadlines are derived from chain state rather than the node's wall clock.
    pub async fn create_proposal(
        &mut self,
        proposer: String,
        title: String,
        description: String,
        proposal_type: ProposalType,
        voting_period: u64,
        deposit: u64,
        current_time: DateTime<Utc>,
    ) -> Result<String> {
        // Validate proposal parameters
        if voting_period < self.params.min_voting_period
            || voting_period > self.params.max_voting_period
        {
            return Err(anyhow::anyhow!("Invalid voting period"));
        }

        if deposit < self.params.min_deposit {
            return Err(anyhow::anyhow!("Deposit too small"));
        }

        // Create the proposal
        let proposal_id = uuid::Uuid::new_v4().to_string();
        let now = current_time;
        let end_time = now + chrono::Duration::seconds(voting_period as i64);

        let proposal = Proposal::new(
            proposal_id.clone(),
            proposer,
            title,
            description,
            proposal_type,
            now,
            end_time,
            deposit,
        );

        // Store the proposal
        self.proposals.insert(proposal_id.clone(), proposal);

        info!("Created new proposal: {}", proposal_id);
        Ok(proposal_id)
    }

    /// Create a DAO parameter update proposal for on-chain controlled parameters.
    ///
    /// `current_time` must be supplied by the caller (typically `block.timestamp`).
    pub async fn create_parameter_update_proposal(
        &mut self,
        proposer: String,
        title: String,
        description: String,
        update: ParameterUpdateProposal,
        voting_period: u64,
        deposit: u64,
        current_time: DateTime<Utc>,
    ) -> Result<String> {
        update.validate()?;
        let (parameter, value) = update.to_parameter_change();
        self.create_proposal(
            proposer,
            title,
            description,
            ProposalType::ParameterChange { parameter, value },
            voting_period,
            deposit,
            current_time,
        )
        .await
    }

    /// Cast a vote on a proposal.
    ///
    /// `current_time` must be supplied by the caller (typically `block.timestamp`) so that
    /// the voting-period check is deterministic across all nodes.
    pub async fn vote(
        &mut self,
        proposal_id: &str,
        voter: String,
        vote: Vote,
        current_time: DateTime<Utc>,
    ) -> Result<()> {
        // Get the proposal
        let proposal = self
            .proposals
            .get_mut(proposal_id)
            .ok_or_else(|| anyhow::anyhow!("Proposal not found"))?;

        // Check if the proposal is active
        if proposal.status != ProposalStatus::Active {
            return Err(anyhow::anyhow!("Proposal is not active"));
        }

        // Check if the voting period has ended
        if current_time > proposal.end_time {
            proposal.status = ProposalStatus::Failed;
            return Err(anyhow::anyhow!("Voting period has ended"));
        }

        // Get the voter's voting power
        let voting_system = self.voting_system.write().await;
        let voting_power = voting_system.get_voting_power(&voter).await?;

        // Cast the vote
        proposal.add_vote(voter, vote, voting_power)?;

        info!("Vote cast on proposal {}", proposal_id);
        Ok(())
    }

    /// Finalize a proposal after the voting period ends.
    ///
    /// `current_time` must be supplied by the caller (typically `block.timestamp`) so that
    /// the finalization check is deterministic across all nodes.
    pub async fn finalize_proposal(
        &mut self,
        proposal_id: &str,
        current_time: DateTime<Utc>,
    ) -> Result<ProposalStatus> {
        // Get the proposal
        let proposal = self
            .proposals
            .get_mut(proposal_id)
            .ok_or_else(|| anyhow::anyhow!("Proposal not found"))?;

        // Check if the proposal can be finalized
        if proposal.status != ProposalStatus::Active {
            return Ok(proposal.status.clone());
        }

        // Check if the voting period has ended
        if current_time <= proposal.end_time {
            return Err(anyhow::anyhow!("Voting period has not ended yet"));
        }

        // Calculate results
        let total_votes = proposal.total_votes();
        let approval_percentage = proposal.approval_percentage();

        // Check quorum
        let quorum_percentage = if self.total_voting_power.value == 0 || total_votes.value == 0 {
            0
        } else {
            (total_votes.value * 100) / self.total_voting_power.value
        };
        if quorum_percentage < self.params.min_quorum as u64 {
            proposal.status = ProposalStatus::Failed;
            info!("Proposal {} failed due to lack of quorum", proposal_id);
            return Ok(proposal.status.clone());
        }

        // Check approval
        if approval_percentage < self.params.min_approval as u64 {
            proposal.status = ProposalStatus::Rejected;
            info!(
                "Proposal {} rejected due to insufficient approval",
                proposal_id
            );
            return Ok(proposal.status.clone());
        }

        // Proposal passed, implement it
        let mut implementation = self.implementation.write().await;
        match implementation.implement_proposal(proposal).await {
            Ok(()) => {
                proposal.status = ProposalStatus::Executed;
                info!("Proposal {} executed successfully", proposal_id);
            },
            Err(e) => {
                proposal.status = ProposalStatus::Failed;
                error!("Failed to implement proposal {}: {}", proposal_id, e);
            },
        }

        Ok(proposal.status.clone())
    }

    /// Get a proposal by ID
    pub fn get_proposal(&self, proposal_id: &str) -> Option<&Proposal> {
        self.proposals.get(proposal_id)
    }

    /// List all proposals
    pub fn list_proposals(&self) -> Vec<&Proposal> {
        self.proposals.values().collect()
    }

    /// List active proposals
    pub fn list_active_proposals(&self) -> Vec<&Proposal> {
        self.proposals
            .values()
            .filter(|p| p.status == ProposalStatus::Active)
            .collect()
    }

    /// Update the total voting power in the system
    pub async fn update_total_voting_power(&mut self) -> Result<()> {
        let voting_system = self.voting_system.read().await;
        self.total_voting_power = voting_system.get_total_voting_power().await?;
        Ok(())
    }
}

/// Initialize the governance system
pub fn init_governance() -> Governance {
    info!("Initializing governance system");

    // Create voting system
    let voting_system = Box::new(voting::TokenBasedVoting::new());

    // Create implementation system
    let implementation = Box::new(implementation::AutomaticImplementation::new());

    // Create governance system
    Governance::new(GovernanceParams::default(), voting_system, implementation)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quality_parameter_preview_builds_protocol_compatible_payload() {
        let preview = build_quality_parameter_preview(
            &DataQualityPolicy::default(),
            ParameterUpdateProposal {
                target: GovernedParameter::QualityWarningThreshold,
                value: "0.58".to_string(),
            },
        )
        .expect("quality proposal preview should be created");

        assert_eq!(preview.parameter, "quality.warning_threshold");
        assert_eq!(preview.value, "0.58");
        assert_eq!(preview.resulting_policy.warning_only_threshold, 0.58);
        assert_eq!(preview.band_outcomes.len(), 4);
    }

    #[test]
    fn quality_parameter_preview_rejects_invalid_threshold_ordering() {
        let err = build_quality_parameter_preview(
            &DataQualityPolicy::default(),
            ParameterUpdateProposal {
                target: GovernedParameter::QualityWarningThreshold,
                value: "0.95".to_string(),
            },
        )
        .expect_err("warning threshold above acceptance threshold should fail");

        assert!(
            err.to_string().contains("invalid quality policy update"),
            "expected quality policy ordering validation error, got {err}"
        );
    }

    #[test]
    fn quality_parameter_validation_rejects_non_positive_slashing_controls() {
        let err = ParameterUpdateProposal {
            target: GovernedParameter::QualityLowSlashStreak,
            value: "0".to_string(),
        }
        .validate()
        .expect_err("zero slash streak must be rejected");
        assert!(err.to_string().contains(">= 1"));

        let err = ParameterUpdateProposal {
            target: GovernedParameter::QualityLowSlashAmount,
            value: "0".to_string(),
        }
        .validate()
        .expect_err("zero slash amount must be rejected");
        assert!(err.to_string().contains(">= 1"));
    }

    #[test]
    fn quality_parameter_preview_rejects_non_quality_target_without_panicking() {
        let err = build_quality_parameter_preview(
            &DataQualityPolicy::default(),
            ParameterUpdateProposal {
                target: GovernedParameter::OracleQuorum,
                value: "2".to_string(),
            },
        )
        .expect_err("non-quality targets should be rejected");
        let typed = err
            .downcast_ref::<GovernancePreviewError>()
            .expect("error should preserve typed governance preview invariant");
        assert_eq!(typed, &GovernancePreviewError::NonQualityTarget);
        assert!(
            err.to_string()
                .contains("SP_GOV_PREVIEW_NON_QUALITY_TARGET")
        );
    }

    #[test]
    fn trust_weight_preview_builds_protocol_compatible_payload() {
        let current = TrustWeightingPolicy {
            unverified_weight: 0.55,
            verified_station_weight: 0.80,
            trusted_provider_weight: 1.0,
        };
        let preview = build_trust_weight_parameter_preview(
            &current,
            ParameterUpdateProposal {
                target: GovernedParameter::TrustVerifiedStationWeight,
                value: "0.85".to_string(),
            },
        )
        .expect("trust preview should build");

        assert_eq!(preview.parameter, "trust.verified_station_weight");
        assert_eq!(preview.value, "0.85");
        assert_eq!(preview.current_policy.verified_station_weight, 0.80);
        assert_eq!(preview.resulting_policy.verified_station_weight, 0.85);
        assert!(preview.ordering_hints.iter().all(|hint| hint.satisfied));
        assert_eq!(preview.class_multipliers.len(), 3);
    }

    #[test]
    fn trust_weight_preview_rejects_invalid_ordering() {
        let err = build_trust_weight_parameter_preview(
            &TrustWeightingPolicy::default(),
            ParameterUpdateProposal {
                target: GovernedParameter::TrustTrustedProviderWeight,
                value: "0.20".to_string(),
            },
        )
        .expect_err("trusted provider weight below verified station should fail");

        assert!(
            err.to_string().contains("invalid trust weighting update"),
            "expected ordering validation error, got {err}"
        );
    }

    #[test]
    fn trust_weight_preview_rejects_non_trust_target_without_panicking() {
        let err = build_trust_weight_parameter_preview(
            &TrustWeightingPolicy::default(),
            ParameterUpdateProposal {
                target: GovernedParameter::QualityWarningThreshold,
                value: "0.58".to_string(),
            },
        )
        .expect_err("non-trust target should be rejected");
        let typed = err
            .downcast_ref::<GovernancePreviewError>()
            .expect("error should preserve typed governance preview invariant");
        assert_eq!(typed, &GovernancePreviewError::NonTrustTarget);
        assert!(err.to_string().contains("SP_GOV_PREVIEW_NON_TRUST_TARGET"));
    }

    #[test]
    fn useful_work_and_multidomain_parameter_updates_validate() {
        ParameterUpdateProposal {
            target: GovernedParameter::PopUsefulWorkActivationHeight,
            value: "0".to_string(),
        }
        .validate()
        .expect("activation height lock should allow 0");

        let err = ParameterUpdateProposal {
            target: GovernedParameter::PopUsefulWorkActivationHeight,
            value: "123".to_string(),
        }
        .validate()
        .expect_err("non-zero activation height must be rejected");
        assert!(err.to_string().contains("must be 0"));

        let err = ParameterUpdateProposal {
            target: GovernedParameter::TaskAssignerUnderservedBonusMultiplier,
            value: "0.8".to_string(),
        }
        .validate()
        .expect_err("underserved bonus < 1.0 must be rejected");
        assert!(err.to_string().contains(">= 1.0"));

        let err = ParameterUpdateProposal {
            target: GovernedParameter::MultidomainSoilWeight,
            value: "-0.1".to_string(),
        }
        .validate()
        .expect_err("negative multidomain weights must be rejected");
        assert!(err.to_string().contains(">= 0"));
    }

    #[test]
    fn useful_work_and_multidomain_parameter_updates_map_to_expected_keys() {
        let pop_activation = ParameterUpdateProposal {
            target: GovernedParameter::PopUsefulWorkActivationHeight,
            value: "42".to_string(),
        }
        .to_parameter_change();
        assert_eq!(pop_activation.0, "pop.useful_work_activation_height");
        assert_eq!(pop_activation.1, "42");

        let readiness_gate = ParameterUpdateProposal {
            target: GovernedParameter::MultidomainSchemaVersionMin,
            value: "2".to_string(),
        }
        .to_parameter_change();
        assert_eq!(readiness_gate.0, "multidomain.schema_version_min");
        assert_eq!(readiness_gate.1, "2");
    }

    #[test]
    fn actor_class_parameter_updates_validate_and_map_to_expected_keys() {
        ParameterUpdateProposal {
            target: GovernedParameter::ActorClassMinAttestations,
            value: "2".to_string(),
        }
        .validate()
        .expect("min attestations should parse");
        let err = ParameterUpdateProposal {
            target: GovernedParameter::ActorClassMinAttestations,
            value: "0".to_string(),
        }
        .validate()
        .expect_err("zero attestations must be rejected");
        assert!(err.to_string().contains(">= 1"));

        ParameterUpdateProposal {
            target: GovernedParameter::ActorClassRegistrationEnabled,
            value: "true".to_string(),
        }
        .validate()
        .expect("registration toggle should parse as bool");
        let err = ParameterUpdateProposal {
            target: GovernedParameter::ActorClassSelfAttestationEnabled,
            value: "not-bool".to_string(),
        }
        .validate()
        .expect_err("invalid actor class toggle must fail");
        assert!(err.to_string().contains("true|false"));

        let min_attestation = ParameterUpdateProposal {
            target: GovernedParameter::ActorClassMinAttestations,
            value: "3".to_string(),
        }
        .to_parameter_change();
        assert_eq!(min_attestation.0, "actor_class.min_attestations");
        assert_eq!(min_attestation.1, "3");

        let toggle = ParameterUpdateProposal {
            target: GovernedParameter::ActorClassRegistrationEnabled,
            value: "false".to_string(),
        }
        .to_parameter_change();
        assert_eq!(toggle.0, "actor_class.registration_enabled");
        assert_eq!(toggle.1, "false");
    }

    #[tokio::test]
    async fn finalize_proposal_zero_voting_power_does_not_panic() {
        let mut gov = init_governance();

        let past = Utc::now() - chrono::Duration::seconds(10);
        let proposal = Proposal::new(
            "test-proposal".to_string(),
            "proposer".to_string(),
            "Test".to_string(),
            "Test zero voting power".to_string(),
            ProposalType::Text {
                content: "signal".to_string(),
            },
            past - chrono::Duration::seconds(100),
            past,
            0,
        );
        gov.proposals.insert("test-proposal".to_string(), proposal);

        // Must not panic (previously would divide by zero at line 857).
        // Tests may pass Utc::now() for current_time; only consensus code must pass block.timestamp.
        let status = gov
            .finalize_proposal("test-proposal", Utc::now())
            .await
            .expect("finalize_proposal should not error");

        assert_eq!(status, ProposalStatus::Failed);
    }

    #[tokio::test]
    async fn governance_proposal_deadline_deterministic() {
        // Verify that create_proposal and vote/finalize use the provided current_time
        // rather than the wall clock, so all nodes with the same block timestamp agree.
        let mut gov = init_governance();

        // Use a fixed "block timestamp" — no wall-clock dependency.
        let block_time =
            chrono::DateTime::<Utc>::from_timestamp(1_700_000_000, 0).expect("valid ts");
        let voting_period_secs = gov.params.min_voting_period;

        let proposal_id = gov
            .create_proposal(
                "proposer".to_string(),
                "Test".to_string(),
                "Deadline determinism".to_string(),
                ProposalType::Text {
                    content: "signal".to_string(),
                },
                voting_period_secs,
                gov.params.min_deposit,
                block_time,
            )
            .await
            .expect("proposal should be created");

        let proposal = gov.proposals.get(&proposal_id).expect("proposal must exist");
        let expected_end = block_time + chrono::Duration::seconds(voting_period_secs as i64);
        assert_eq!(
            proposal.end_time, expected_end,
            "end_time must equal block_time + voting_period"
        );

        // Voting during the period should succeed; finalize before end should error.
        let still_active = block_time + chrono::Duration::seconds(1);
        assert!(
            gov.finalize_proposal(&proposal_id, still_active).await.is_err(),
            "finalize before end_time must fail"
        );
    }

    #[tokio::test]
    async fn governance_vote_expiry_deterministic() {
        // Verify that vote() rejects votes after the deterministic end_time.
        let mut gov = init_governance();

        let block_time =
            chrono::DateTime::<Utc>::from_timestamp(1_700_000_000, 0).expect("valid ts");
        let voting_period_secs = gov.params.min_voting_period;

        let proposal_id = gov
            .create_proposal(
                "proposer".to_string(),
                "Test".to_string(),
                "Vote expiry".to_string(),
                ProposalType::Text {
                    content: "signal".to_string(),
                },
                voting_period_secs,
                gov.params.min_deposit,
                block_time,
            )
            .await
            .expect("proposal should be created");

        // Voting exactly at end_time is already expired.
        let end_time = block_time + chrono::Duration::seconds(voting_period_secs as i64);
        let result = gov
            .vote(
                &proposal_id,
                "voter".to_string(),
                Vote::Yes,
                end_time + chrono::Duration::seconds(1),
            )
            .await;
        assert!(result.is_err(), "vote after end_time must be rejected");
    }
}
