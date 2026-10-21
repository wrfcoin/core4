//! Governance implementation mechanisms
//!
//! This module provides the implementation mechanisms for governance proposals.

use crate::governance::proposal::{Proposal, ProposalType};
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use log::info;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Interface for governance implementation mechanisms
#[async_trait]
pub trait GovernanceImplementor: Send + Sync {
    /// Implement a successful proposal
    async fn implement_proposal(&mut self, proposal: &Proposal) -> Result<()>;

    /// Check the implementation status of a proposal
    async fn check_implementation_status(&self, proposal_id: &str) -> Result<ImplementationStatus>;
}

/// Status of proposal implementation
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImplementationStatus {
    /// Proposal has not been implemented yet
    NotImplemented,

    /// Proposal is being implemented
    InProgress,

    /// Proposal has been fully implemented
    Implemented,

    /// Implementation failed
    Failed(String),
}

/// Enum representing the proposal action to be taken
#[derive(Debug)]
enum ProposalAction {
    ParameterChange(String, String),
    ProtocolUpgrade(String, String),
    FundingProposal(String, u64),
    Text,
}

/// Automatic implementation mechanism
pub struct AutomaticImplementation {
    /// Parameters that can be changed via governance
    parameters: HashMap<String, String>,

    /// Protocol versions
    protocol_versions: HashMap<String, String>,

    /// Implementation status of proposals
    implementation_status: HashMap<String, ImplementationStatus>,

    /// Lock for synchronization
    lock: Arc<Mutex<()>>,
}

impl AutomaticImplementation {
    /// Create a new automatic implementation mechanism
    pub fn new() -> Self {
        Self {
            parameters: HashMap::new(),
            protocol_versions: HashMap::new(),
            implementation_status: HashMap::new(),
            lock: Arc::new(Mutex::new(())),
        }
    }

    /// Set a parameter value
    pub fn set_parameter(&mut self, name: String, value: String) {
        self.parameters.insert(name, value);
    }

    /// Get a parameter value
    pub fn get_parameter(&self, name: &str) -> Option<String> {
        self.parameters.get(name).cloned()
    }

    /// Add a protocol version
    pub fn add_protocol_version(&mut self, version: String, code_hash: String) {
        self.protocol_versions.insert(version, code_hash);
    }

    /// Get a protocol version code hash
    pub fn get_protocol_version(&self, version: &str) -> Option<String> {
        self.protocol_versions.get(version).cloned()
    }
}

#[async_trait]
impl GovernanceImplementor for AutomaticImplementation {
    async fn implement_proposal(&mut self, proposal: &Proposal) -> Result<()> {
        // First, check if proposal has already been implemented
        {
            let _status_guard = self.lock.lock().await;
            if let Some(status) = self.implementation_status.get(&proposal.id) {
                if status == &ImplementationStatus::Implemented {
                    return Ok(());
                }
            }
            // Update status to in progress while still in this scope
            self.implementation_status
                .insert(proposal.id.clone(), ImplementationStatus::InProgress);
            // _status_guard is dropped here, releasing the lock
        }

        // Clone the data we need outside the lock to avoid borrow checker issues
        let proposal_action = match &proposal.proposal_type {
            ProposalType::ParameterChange { parameter, value } => {
                let param = parameter.clone();
                let val = value.clone();
                ProposalAction::ParameterChange(param, val)
            },

            ProposalType::ProtocolUpgrade {
                version,
                code_hash,
                activation_height: _,
            } => {
                let ver = version.clone();
                let hash = code_hash.clone();
                ProposalAction::ProtocolUpgrade(ver, hash)
            },

            ProposalType::FundingProposal {
                recipient,
                amount,
                purpose: _,
            } => {
                let recip = recipient.clone();
                let amt = *amount;
                ProposalAction::FundingProposal(recip, amt)
            },

            ProposalType::Text { content: _ } => ProposalAction::Text,
        };

        // Implement the proposal based on its action
        match proposal_action {
            ProposalAction::ParameterChange(parameter, value) => {
                info!("Implementing parameter change: {} = {}", parameter, value);
                self.set_parameter(parameter, value);
            },

            ProposalAction::ProtocolUpgrade(version, code_hash) => {
                info!("Implementing protocol upgrade to version {}", version);
                self.add_protocol_version(version, code_hash);
                // Protocol upgrades typically require more complex logic in a real implementation
            },

            ProposalAction::FundingProposal(recipient, amount) => {
                info!(
                    "Implementing funding proposal: {} tokens to {}",
                    amount, recipient
                );
                // Funding proposals would actually transfer tokens in a real implementation
                // For now, we just log the action
            },

            ProposalAction::Text => {
                // Text proposals don't require implementation
                info!("Text proposal does not require implementation");
            },
        }

        // Update status to implemented
        self.implementation_status
            .insert(proposal.id.clone(), ImplementationStatus::Implemented);

        Ok(())
    }

    async fn check_implementation_status(&self, proposal_id: &str) -> Result<ImplementationStatus> {
        self.implementation_status
            .get(proposal_id)
            .cloned()
            .ok_or_else(|| anyhow!("No implementation status found for proposal"))
    }
}

/// Delayed implementation mechanism
#[cfg(feature = "governance")]
pub struct DelayedImplementation {
    /// Inner implementation
    inner: AutomaticImplementation,

    /// Proposals queued for implementation
    implementation_queue: Vec<(Proposal, u64)>, // (Proposal, implementation height)

    /// Current blockchain height
    current_height: u64,
}

#[cfg(feature = "governance")]
impl DelayedImplementation {
    /// Create a new delayed implementation mechanism
    pub fn new(delay_blocks: u64) -> Self {
        Self {
            inner: AutomaticImplementation::new(),
            implementation_queue: Vec::new(),
            current_height: 0,
        }
    }

    /// Update the current blockchain height
    pub fn update_height(&mut self, height: u64) {
        self.current_height = height;
    }

    /// Process the implementation queue
    pub async fn process_queue(&mut self) -> Result<Vec<String>> {
        let mut implemented = Vec::new();

        // Find proposals that are ready to be implemented
        let ready_proposals = self
            .implementation_queue
            .iter()
            .filter(|(_, height)| *height <= self.current_height)
            .map(|(proposal, _)| proposal.id.clone())
            .collect::<Vec<_>>();

        // Implement each ready proposal
        for proposal_id in &ready_proposals {
            if let Some(pos) = self
                .implementation_queue
                .iter()
                .position(|(p, _)| p.id == *proposal_id)
            {
                let (proposal, _) = self.implementation_queue.remove(pos);
                self.inner.implement_proposal(&proposal).await?;
                implemented.push(proposal_id.clone());
            }
        }

        Ok(implemented)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::governance::proposal::{Proposal, ProposalStatus, ProposalType};
    use chrono::Utc;

    fn text_proposal(id: &str, proposer: &str) -> Proposal {
        let now = Utc::now();
        Proposal::new(
            id.to_string(),
            proposer.to_string(),
            "title".to_string(),
            "desc".to_string(),
            ProposalType::Text { content: "signal".to_string() },
            now,
            now + chrono::Duration::days(7),
            0,
        )
    }

    fn param_proposal(id: &str, param: &str, value: &str) -> Proposal {
        let now = Utc::now();
        Proposal::new(
            id.to_string(),
            "proposer".to_string(),
            "param change".to_string(),
            "desc".to_string(),
            ProposalType::ParameterChange {
                parameter: param.to_string(),
                value: value.to_string(),
            },
            now,
            now + chrono::Duration::days(7),
            0,
        )
    }

    #[test]
    fn set_and_get_parameter_roundtrip() {
        let mut imp = AutomaticImplementation::new();
        imp.set_parameter("max_tx_per_block".to_string(), "500".to_string());
        assert_eq!(imp.get_parameter("max_tx_per_block"), Some("500".to_string()));
        assert_eq!(imp.get_parameter("unknown"), None);
    }

    #[test]
    fn set_parameter_overwrites_previous() {
        let mut imp = AutomaticImplementation::new();
        imp.set_parameter("fee".to_string(), "100".to_string());
        imp.set_parameter("fee".to_string(), "200".to_string());
        assert_eq!(imp.get_parameter("fee"), Some("200".to_string()));
    }

    #[test]
    fn add_and_get_protocol_version() {
        let mut imp = AutomaticImplementation::new();
        imp.add_protocol_version("v2.0".to_string(), "deadbeef".to_string());
        assert_eq!(imp.get_protocol_version("v2.0"), Some("deadbeef".to_string()));
        assert_eq!(imp.get_protocol_version("v3.0"), None);
    }

    #[tokio::test]
    async fn implement_parameter_change_proposal_applies_param() {
        let mut imp = AutomaticImplementation::new();
        let proposal = param_proposal("p1", "block_time", "12");
        imp.implement_proposal(&proposal).await.unwrap();

        assert_eq!(imp.get_parameter("block_time"), Some("12".to_string()));

        let status = imp.check_implementation_status("p1").await.unwrap();
        assert_eq!(status, ImplementationStatus::Implemented);
    }

    #[tokio::test]
    async fn implement_text_proposal_marks_implemented() {
        let mut imp = AutomaticImplementation::new();
        let proposal = text_proposal("p2", "alice");
        imp.implement_proposal(&proposal).await.unwrap();

        let status = imp.check_implementation_status("p2").await.unwrap();
        assert_eq!(status, ImplementationStatus::Implemented);
    }

    #[tokio::test]
    async fn implement_same_proposal_twice_is_idempotent() {
        let mut imp = AutomaticImplementation::new();
        let proposal = param_proposal("p3", "fee", "99");
        imp.implement_proposal(&proposal).await.unwrap();
        // Second call should succeed without double-applying
        imp.implement_proposal(&proposal).await.unwrap();

        assert_eq!(imp.get_parameter("fee"), Some("99".to_string()));
        let status = imp.check_implementation_status("p3").await.unwrap();
        assert_eq!(status, ImplementationStatus::Implemented);
    }

    #[tokio::test]
    async fn check_status_unknown_proposal_returns_error() {
        let imp = AutomaticImplementation::new();
        assert!(imp.check_implementation_status("nonexistent").await.is_err());
    }
}

#[cfg(feature = "governance")]
#[async_trait]
impl GovernanceImplementor for DelayedImplementation {
    async fn implement_proposal(&mut self, proposal: &Proposal) -> Result<()> {
        // Queue the proposal for implementation at a future height
        let implementation_height = self.current_height + 100; // Example: implement after 100 blocks
        self.implementation_queue
            .push((proposal.clone(), implementation_height));

        Ok(())
    }

    async fn check_implementation_status(&self, proposal_id: &str) -> Result<ImplementationStatus> {
        // First check if it's in the inner implementation's status
        if let Ok(status) = self.inner.check_implementation_status(proposal_id).await {
            return Ok(status);
        }

        // Then check if it's in the queue
        if let Some((_, height)) = self
            .implementation_queue
            .iter()
            .find(|(p, _)| p.id == proposal_id)
        {
            return Ok(ImplementationStatus::InProgress);
        }

        Ok(ImplementationStatus::NotImplemented)
    }
}
