# Wrfcoin Governance Implementation Guide

This document provides detailed implementation guidance for the first phase of our governance
expansion plan. We'll focus on the most impactful features that can be implemented quickly.

## Phase 1 Implementation: Core Governance Infrastructure

### 1. Basic Proposal System

#### Data Models

First, let's define the data models for governance proposals:

```rust
// src/governance/models.rs

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProposalType {
    ProtocolUpgrade,
    ParameterChange,
    FundingRequest,
    CommunityInitiative,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProposalStatus {
    Draft,
    Active,
    Passed,
    Rejected,
    Executed,
    Expired,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProposalMetadata {
    pub title: String,
    pub description: String,
    pub rationale: String,
    pub timeline: u64, // Timeline in seconds
    pub created_at: u64,
    pub additional_fields: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proposal {
    pub id: String,
    pub proposal_type: ProposalType,
    pub status: ProposalStatus,
    pub metadata: ProposalMetadata,
    pub proposer: String, // Account ID
    pub content: String,  // JSON encoded content specific to proposal type
    pub voting_start: u64,
    pub voting_end: u64,
    pub execution_delay: Option<u64>,
    pub votes_for: u64,
    pub votes_against: u64,
    pub votes_abstain: u64,
}
```

#### Storage Implementation

Next, implement storage for proposals:

```rust
// src/governance/storage.rs

use super::models::{Proposal, ProposalStatus};
use crate::core::error::WrfError;
use std::collections::HashMap;

pub struct ProposalStorage {
    proposals: HashMap<String, Proposal>,
}

impl ProposalStorage {
    pub fn new() -> Self {
        Self {
            proposals: HashMap::new(),
        }
    }

    pub fn add_proposal(&mut self, proposal: Proposal) -> Result<(), WrfError> {
        if self.proposals.contains_key(&proposal.id) {
            return Err(WrfError::AlreadyExists("Proposal already exists".to_string()));
        }
        self.proposals.insert(proposal.id.clone(), proposal);
        Ok(())
    }

    pub fn get_proposal(&self, id: &str) -> Option<&Proposal> {
        self.proposals.get(id)
    }

    pub fn get_proposal_mut(&mut self, id: &str) -> Option<&mut Proposal> {
        self.proposals.get_mut(id)
    }

    pub fn update_proposal_status(&mut self, id: &str, status: ProposalStatus) -> Result<(), WrfError> {
        match self.proposals.get_mut(id) {
            Some(proposal) => {
                proposal.status = status;
                Ok(())
            }
            None => Err(WrfError::NotFound("Proposal not found".to_string())),
        }
    }

    pub fn list_proposals(&self) -> Vec<&Proposal> {
        self.proposals.values().collect()
    }

    pub fn list_proposals_by_status(&self, status: ProposalStatus) -> Vec<&Proposal> {
        self.proposals
            .values()
            .filter(|p| p.status == status)
            .collect()
    }
}
```

#### Core Governance Logic

Implement the governance manager:

```rust
// src/governance/manager.rs

use super::models::{Proposal, ProposalStatus, ProposalType};
use super::storage::ProposalStorage;
use crate::blockchain::state::account::Account;
use crate::core::error::WrfError;
use uuid::Uuid;

pub struct GovernanceManager {
    proposal_storage: ProposalStorage,
}

impl GovernanceManager {
    pub fn new() -> Self {
        Self {
            proposal_storage: ProposalStorage::new(),
        }
    }

    pub fn create_proposal(
        &mut self,
        proposer: &Account,
        proposal_type: ProposalType,
        title: String,
        description: String,
        rationale: String,
        content: String,
        voting_duration: u64,
        execution_delay: Option<u64>,
    ) -> Result<String, WrfError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let proposal_id = Uuid::new_v4().to_string();

        let proposal = Proposal {
            id: proposal_id.clone(),
            proposal_type,
            status: ProposalStatus::Draft,
            metadata: super::models::ProposalMetadata {
                title,
                description,
                rationale,
                timeline: voting_duration,
                created_at: now,
                additional_fields: std::collections::HashMap::new(),
            },
            proposer: proposer.id.clone(),
            content,
            voting_start: now,
            voting_end: now + voting_duration,
            execution_delay,
            votes_for: 0,
            votes_against: 0,
            votes_abstain: 0,
        };

        self.proposal_storage.add_proposal(proposal)?;
        Ok(proposal_id)
    }

    pub fn activate_proposal(&mut self, proposal_id: &str) -> Result<(), WrfError> {
        let proposal = self
            .proposal_storage
            .get_proposal(proposal_id)
            .ok_or_else(|| WrfError::NotFound("Proposal not found".to_string()))?;

        if proposal.status != ProposalStatus::Draft {
            return Err(WrfError::InvalidState(
                "Proposal is not in draft state".to_string(),
            ));
        }

        self.proposal_storage
            .update_proposal_status(proposal_id, ProposalStatus::Active)
    }

    pub fn vote_on_proposal(
        &mut self,
        voter: &Account,
        proposal_id: &str,
        vote_for: bool,
        vote_abstain: bool,
    ) -> Result<(), WrfError> {
        let proposal = self
            .proposal_storage
            .get_proposal_mut(proposal_id)
            .ok_or_else(|| WrfError::NotFound("Proposal not found".to_string()))?;

        if proposal.status != ProposalStatus::Active {
            return Err(WrfError::InvalidState(
                "Proposal is not active".to_string(),
            ));
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        if now > proposal.voting_end {
            return Err(WrfError::InvalidState(
                "Voting period has ended".to_string(),
            ));
        }

        // Simple token-weighted voting based on account balance
        let voting_power = voter.balance;

        if vote_abstain {
            proposal.votes_abstain += voting_power;
        } else if vote_for {
            proposal.votes_for += voting_power;
        } else {
            proposal.votes_against += voting_power;
        }

        Ok(())
    }

    pub fn finalize_proposal(&mut self, proposal_id: &str) -> Result<(), WrfError> {
        let proposal = self
            .proposal_storage
            .get_proposal(proposal_id)
            .ok_or_else(|| WrfError::NotFound("Proposal not found".to_string()))?;

        if proposal.status != ProposalStatus::Active {
            return Err(WrfError::InvalidState(
                "Proposal is not active".to_string(),
            ));
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        if now < proposal.voting_end {
            return Err(WrfError::InvalidState(
                "Voting period has not ended".to_string(),
            ));
        }

        // Simple majority voting
        let new_status = if proposal.votes_for > proposal.votes_against {
            ProposalStatus::Passed
        } else {
            ProposalStatus::Rejected
        };

        self.proposal_storage
            .update_proposal_status(proposal_id, new_status)
    }

    pub fn execute_proposal(&mut self, proposal_id: &str) -> Result<(), WrfError> {
        let proposal = self
            .proposal_storage
            .get_proposal(proposal_id)
            .ok_or_else(|| WrfError::NotFound("Proposal not found".to_string()))?;

        if proposal.status != ProposalStatus::Passed {
            return Err(WrfError::InvalidState(
                "Proposal has not passed".to_string(),
            ));
        }

        // Check execution delay if it exists
        if let Some(delay) = proposal.execution_delay {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();

            if now < proposal.voting_end + delay {
                return Err(WrfError::InvalidState(
                    "Execution delay has not passed".to_string(),
                ));
            }
        }

        // Here we would execute the proposal based on its type
        // For now, we just mark it as executed
        self.proposal_storage
            .update_proposal_status(proposal_id, ProposalStatus::Executed)
    }
}
```

### 2. Simple Treasury Management

#### Treasury Data Models

```rust
// src/governance/treasury.rs

use serde::{Deserialize, Serialize};
use crate::core::error::WrfError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TreasuryTransactionType {
    Deposit,
    Withdrawal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreasuryTransaction {
    pub id: String,
    pub transaction_type: TreasuryTransactionType,
    pub amount: u64,
    pub sender: String,
    pub receiver: Option<String>,
    pub proposal_id: Option<String>,
    pub timestamp: u64,
    pub description: String,
}

pub struct Treasury {
    pub balance: u64,
    pub transactions: Vec<TreasuryTransaction>,
    pub signatories: Vec<String>, // Account IDs of treasury signatories
    pub required_signatures: usize,
}

impl Treasury {
    pub fn new(signatories: Vec<String>, required_signatures: usize) -> Self {
        Self {
            balance: 0,
            transactions: Vec::new(),
            signatories,
            required_signatures,
        }
    }

    pub fn deposit(&mut self, amount: u64, sender: String, description: String) -> Result<(), WrfError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let transaction = TreasuryTransaction {
            id: uuid::Uuid::new_v4().to_string(),
            transaction_type: TreasuryTransactionType::Deposit,
            amount,
            sender,
            receiver: None,
            proposal_id: None,
            timestamp: now,
            description,
        };

        self.balance += amount;
        self.transactions.push(transaction);
        Ok(())
    }

    pub fn withdraw(
        &mut self,
        amount: u64,
        receiver: String,
        proposal_id: String,
        description: String,
        signatories: &[String],
    ) -> Result<(), WrfError> {
        // Check if enough signatories
        if signatories.len() < self.required_signatures {
            return Err(WrfError::Unauthorized(
                "Not enough signatures for withdrawal".to_string(),
            ));
        }

        // Verify all signatories are valid
        for signatory in signatories {
            if !self.signatories.contains(signatory) {
                return Err(WrfError::Unauthorized(
                    format!("Invalid signatory: {}", signatory),
                ));
            }
        }

        if amount > self.balance {
            return Err(WrfError::InsufficientFunds(
                "Treasury has insufficient funds".to_string(),
            ));
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let transaction = TreasuryTransaction {
            id: uuid::Uuid::new_v4().to_string(),
            transaction_type: TreasuryTransactionType::Withdrawal,
            amount,
            sender: "treasury".to_string(),
            receiver: Some(receiver),
            proposal_id: Some(proposal_id),
            timestamp: now,
            description,
        };

        self.balance -= amount;
        self.transactions.push(transaction);
        Ok(())
    }

    pub fn get_transaction_history(&self) -> &[TreasuryTransaction] {
        &self.transactions
    }
}
```

### 3. Governance Module Setup

Create the module structure:

```rust
// src/governance/mod.rs

pub mod manager;
pub mod models;
pub mod storage;
pub mod treasury;

pub use manager::GovernanceManager;
pub use models::{Proposal, ProposalMetadata, ProposalStatus, ProposalType};
pub use treasury::{Treasury, TreasuryTransaction, TreasuryTransactionType};
```

## First Phase UI Components

For the first phase, let's sketch some UI components for the web interface:

### Proposal List Component

```jsx
// src/components/governance/ProposalList.jsx

import React, { useState, useEffect } from 'react';
import { Box, Card, CardContent, Typography, Button, Chip, Grid } from '@mui/material';
import { useNavigate } from 'react-router-dom';
import api from '../../services/api';

const ProposalList = () => {
  const [proposals, setProposals] = useState([]);
  const [loading, setLoading] = useState(true);
  const navigate = useNavigate();

  useEffect(() => {
    const fetchProposals = async () => {
      try {
        const response = await api.getProposals();
        setProposals(response.data);
        setLoading(false);
      } catch (error) {
        console.error('Error fetching proposals:', error);
        setLoading(false);
      }
    };

    fetchProposals();
  }, []);

  const getStatusColor = status => {
    switch (status) {
      case 'Draft':
        return 'default';
      case 'Active':
        return 'primary';
      case 'Passed':
        return 'success';
      case 'Rejected':
        return 'error';
      case 'Executed':
        return 'secondary';
      case 'Expired':
        return 'warning';
      default:
        return 'default';
    }
  };

  const handleViewProposal = id => {
    navigate(`/governance/proposals/${id}`);
  };

  return (
    <Box>
      <Box sx={{ display: 'flex', justifyContent: 'space-between', mb: 3 }}>
        <Typography variant="h4">Governance Proposals</Typography>
        <Button
          variant="contained"
          color="primary"
          onClick={() => navigate('/governance/proposals/create')}
        >
          Create Proposal
        </Button>
      </Box>

      {loading ? (
        <Typography>Loading proposals...</Typography>
      ) : proposals.length === 0 ? (
        <Typography>No proposals found.</Typography>
      ) : (
        <Grid container spacing={3}>
          {proposals.map(proposal => (
            <Grid item xs={12} key={proposal.id}>
              <Card>
                <CardContent>
                  <Box sx={{ display: 'flex', justifyContent: 'space-between', mb: 1 }}>
                    <Typography variant="h6">{proposal.metadata.title}</Typography>
                    <Chip label={proposal.status} color={getStatusColor(proposal.status)} />
                  </Box>
                  <Typography color="textSecondary" gutterBottom>
                    Proposal ID: {proposal.id}
                  </Typography>
                  <Typography variant="body2" sx={{ mb: 2 }}>
                    {proposal.metadata.description.substring(0, 150)}...
                  </Typography>
                  <Box sx={{ display: 'flex', justifyContent: 'space-between' }}>
                    <Typography variant="body2" color="textSecondary">
                      Proposed by: {proposal.proposer}
                    </Typography>
                    <Button
                      variant="outlined"
                      size="small"
                      onClick={() => handleViewProposal(proposal.id)}
                    >
                      View Details
                    </Button>
                  </Box>
                </CardContent>
              </Card>
            </Grid>
          ))}
        </Grid>
      )}
    </Box>
  );
};

export default ProposalList;
```

### Proposal Detail Component

```jsx
// src/components/governance/ProposalDetail.jsx

import React, { useState, useEffect } from 'react';
import { useParams } from 'react-router-dom';
import {
  Box,
  Card,
  CardContent,
  Typography,
  Button,
  Chip,
  Grid,
  LinearProgress,
  Divider,
} from '@mui/material';
import { ThumbUp, ThumbDown, HorizontalRule } from '@mui/icons-material';
import api from '../../services/api';

const ProposalDetail = () => {
  const { id } = useParams();
  const [proposal, setProposal] = useState(null);
  const [loading, setLoading] = useState(true);
  const [userVote, setUserVote] = useState(null);
  const [votingDisabled, setVotingDisabled] = useState(false);

  useEffect(() => {
    const fetchProposal = async () => {
      try {
        const response = await api.getProposal(id);
        setProposal(response.data);

        // Check if voting is still open
        const now = Math.floor(Date.now() / 1000);
        setVotingDisabled(response.data.status !== 'Active' || now > response.data.voting_end);

        setLoading(false);
      } catch (error) {
        console.error('Error fetching proposal:', error);
        setLoading(false);
      }
    };

    fetchProposal();
  }, [id]);

  const handleVote = async voteType => {
    try {
      await api.voteOnProposal(id, voteType);
      setUserVote(voteType);

      // Refresh proposal data to get updated vote counts
      const response = await api.getProposal(id);
      setProposal(response.data);
    } catch (error) {
      console.error('Error voting on proposal:', error);
    }
  };

  const getStatusColor = status => {
    switch (status) {
      case 'Draft':
        return 'default';
      case 'Active':
        return 'primary';
      case 'Passed':
        return 'success';
      case 'Rejected':
        return 'error';
      case 'Executed':
        return 'secondary';
      case 'Expired':
        return 'warning';
      default:
        return 'default';
    }
  };

  const calculateVotePercentages = () => {
    const total = proposal.votes_for + proposal.votes_against + proposal.votes_abstain;
    if (total === 0) return { for: 0, against: 0, abstain: 0 };

    return {
      for: (proposal.votes_for / total) * 100,
      against: (proposal.votes_against / total) * 100,
      abstain: (proposal.votes_abstain / total) * 100,
    };
  };

  if (loading) {
    return <Typography>Loading proposal details...</Typography>;
  }

  if (!proposal) {
    return <Typography>Proposal not found.</Typography>;
  }

  const votePercentages = calculateVotePercentages();
  const votingEndDate = new Date(proposal.voting_end * 1000).toLocaleString();

  return (
    <Box>
      <Card sx={{ mb: 4 }}>
        <CardContent>
          <Box sx={{ display: 'flex', justifyContent: 'space-between', mb: 2 }}>
            <Typography variant="h4">{proposal.metadata.title}</Typography>
            <Chip
              label={proposal.status}
              color={getStatusColor(proposal.status)}
              sx={{ height: 30 }}
            />
          </Box>

          <Typography color="textSecondary" gutterBottom>
            Proposal ID: {proposal.id}
          </Typography>

          <Typography variant="body1" sx={{ mt: 3, mb: 3 }}>
            {proposal.metadata.description}
          </Typography>

          <Divider sx={{ my: 3 }} />

          <Typography variant="h6" sx={{ mb: 2 }}>
            Rationale
          </Typography>
          <Typography variant="body1" sx={{ mb: 3 }}>
            {proposal.metadata.rationale}
          </Typography>

          <Divider sx={{ my: 3 }} />

          <Grid container spacing={2}>
            <Grid item xs={12} md={6}>
              <Typography variant="subtitle1">Proposed by</Typography>
              <Typography variant="body2">{proposal.proposer}</Typography>
            </Grid>
            <Grid item xs={12} md={6}>
              <Typography variant="subtitle1">Voting Period Ends</Typography>
              <Typography variant="body2">{votingEndDate}</Typography>
            </Grid>
          </Grid>
        </CardContent>
      </Card>

      <Card>
        <CardContent>
          <Typography variant="h5" sx={{ mb: 3 }}>
            Vote
          </Typography>

          <Grid container spacing={3} sx={{ mb: 4 }}>
            <Grid item xs={4}>
              <Button
                variant={userVote === 'for' ? 'contained' : 'outlined'}
                color="success"
                fullWidth
                startIcon={<ThumbUp />}
                onClick={() => handleVote('for')}
                disabled={votingDisabled}
              >
                Vote For
              </Button>
            </Grid>
            <Grid item xs={4}>
              <Button
                variant={userVote === 'against' ? 'contained' : 'outlined'}
                color="error"
                fullWidth
                startIcon={<ThumbDown />}
                onClick={() => handleVote('against')}
                disabled={votingDisabled}
              >
                Vote Against
              </Button>
            </Grid>
            <Grid item xs={4}>
              <Button
                variant={userVote === 'abstain' ? 'contained' : 'outlined'}
                color="info"
                fullWidth
                startIcon={<HorizontalRule />}
                onClick={() => handleVote('abstain')}
                disabled={votingDisabled}
              >
                Abstain
              </Button>
            </Grid>
          </Grid>

          <Typography variant="h6" sx={{ mb: 2 }}>
            Current Results
          </Typography>

          <Box sx={{ mb: 1 }}>
            <Box sx={{ display: 'flex', justifyContent: 'space-between' }}>
              <Typography variant="body2">For</Typography>
              <Typography variant="body2">{votePercentages.for.toFixed(2)}%</Typography>
            </Box>
            <LinearProgress
              variant="determinate"
              value={votePercentages.for}
              color="success"
              sx={{ height: 10, borderRadius: 5, my: 1 }}
            />
          </Box>

          <Box sx={{ mb: 1 }}>
            <Box sx={{ display: 'flex', justifyContent: 'space-between' }}>
              <Typography variant="body2">Against</Typography>
              <Typography variant="body2">{votePercentages.against.toFixed(2)}%</Typography>
            </Box>
            <LinearProgress
              variant="determinate"
              value={votePercentages.against}
              color="error"
              sx={{ height: 10, borderRadius: 5, my: 1 }}
            />
          </Box>

          <Box>
            <Box sx={{ display: 'flex', justifyContent: 'space-between' }}>
              <Typography variant="body2">Abstain</Typography>
              <Typography variant="body2">{votePercentages.abstain.toFixed(2)}%</Typography>
            </Box>
            <LinearProgress
              variant="determinate"
              value={votePercentages.abstain}
              color="info"
              sx={{ height: 10, borderRadius: 5, my: 1 }}
            />
          </Box>

          <Box sx={{ mt: 3 }}>
            <Typography variant="body2">
              Total Votes: {proposal.votes_for + proposal.votes_against + proposal.votes_abstain}
            </Typography>
          </Box>
        </CardContent>
      </Card>
    </Box>
  );
};

export default ProposalDetail;
```

## API Implementation

Next, let's implement the API endpoints for our governance system:

```javascript
// server/routes/governance.js

const express = require('express');
const router = express.Router();
const { authenticateToken } = require('../middleware/authMiddleware');
const governanceController = require('../controllers/governanceController');

// Proposal routes
router.get('/proposals', governanceController.getProposals);
router.get('/proposals/:id', governanceController.getProposalById);
router.post('/proposals', authenticateToken, governanceController.createProposal);
router.post('/proposals/:id/vote', authenticateToken, governanceController.voteOnProposal);
router.post('/proposals/:id/activate', authenticateToken, governanceController.activateProposal);
router.post('/proposals/:id/finalize', authenticateToken, governanceController.finalizeProposal);
router.post('/proposals/:id/execute', authenticateToken, governanceController.executeProposal);

// Treasury routes
router.get('/treasury', governanceController.getTreasuryInfo);
router.get('/treasury/transactions', governanceController.getTreasuryTransactions);
router.post('/treasury/withdraw', authenticateToken, governanceController.withdrawFromTreasury);

module.exports = router;
```

```javascript
// server/controllers/governanceController.js

const { GovernanceManager } = require('../../src/governance/manager');
const { Treasury } = require('../../src/governance/treasury');
const { ProposalType, ProposalStatus } = require('../../src/governance/models');
const { getAccountByAddress } = require('../services/accountService');

// Initialize governance manager and treasury
const governanceManager = new GovernanceManager();
const treasury = new Treasury(
  ['admin1', 'admin2', 'admin3'], // Initial signatories
  2 // Required signatures for withdrawals
);

// Proposal controllers
exports.getProposals = async (req, res) => {
  try {
    const proposals = governanceManager.proposal_storage.list_proposals();
    res.json(proposals);
  } catch (error) {
    res.status(500).json({ error: error.message });
  }
};

exports.getProposalById = async (req, res) => {
  try {
    const proposal = governanceManager.proposal_storage.get_proposal(req.params.id);
    if (!proposal) {
      return res.status(404).json({ error: 'Proposal not found' });
    }
    res.json(proposal);
  } catch (error) {
    res.status(500).json({ error: error.message });
  }
};

exports.createProposal = async (req, res) => {
  try {
    const { proposalType, title, description, rationale, content, votingDuration, executionDelay } =
      req.body;

    const account = await getAccountByAddress(req.user.address);
    if (!account) {
      return res.status(404).json({ error: 'Account not found' });
    }

    const proposalTypeEnum = ProposalType[proposalType];
    if (!proposalTypeEnum) {
      return res.status(400).json({ error: 'Invalid proposal type' });
    }

    const proposalId = await governanceManager.create_proposal(
      account,
      proposalTypeEnum,
      title,
      description,
      rationale,
      content,
      votingDuration,
      executionDelay ? executionDelay : undefined
    );

    res.status(201).json({ id: proposalId });
  } catch (error) {
    res.status(500).json({ error: error.message });
  }
};

exports.voteOnProposal = async (req, res) => {
  try {
    const { voteType } = req.body;
    const { id } = req.params;

    const account = await getAccountByAddress(req.user.address);
    if (!account) {
      return res.status(404).json({ error: 'Account not found' });
    }

    let voteFor = false;
    let voteAbstain = false;

    switch (voteType) {
      case 'for':
        voteFor = true;
        break;
      case 'against':
        voteFor = false;
        break;
      case 'abstain':
        voteAbstain = true;
        break;
      default:
        return res.status(400).json({ error: 'Invalid vote type' });
    }

    await governanceManager.vote_on_proposal(account, id, voteFor, voteAbstain);
    res.json({ success: true });
  } catch (error) {
    res.status(500).json({ error: error.message });
  }
};

exports.activateProposal = async (req, res) => {
  try {
    const { id } = req.params;
    await governanceManager.activate_proposal(id);
    res.json({ success: true });
  } catch (error) {
    res.status(500).json({ error: error.message });
  }
};

exports.finalizeProposal = async (req, res) => {
  try {
    const { id } = req.params;
    await governanceManager.finalize_proposal(id);
    res.json({ success: true });
  } catch (error) {
    res.status(500).json({ error: error.message });
  }
};

exports.executeProposal = async (req, res) => {
  try {
    const { id } = req.params;
    await governanceManager.execute_proposal(id);
    res.json({ success: true });
  } catch (error) {
    res.status(500).json({ error: error.message });
  }
};

// Treasury controllers
exports.getTreasuryInfo = async (req, res) => {
  try {
    res.json({
      balance: treasury.balance,
      signatories: treasury.signatories,
      requiredSignatures: treasury.required_signatures,
    });
  } catch (error) {
    res.status(500).json({ error: error.message });
  }
};

exports.getTreasuryTransactions = async (req, res) => {
  try {
    const transactions = treasury.get_transaction_history();
    res.json(transactions);
  } catch (error) {
    res.status(500).json({ error: error.message });
  }
};

exports.withdrawFromTreasury = async (req, res) => {
  try {
    const { amount, receiver, proposalId, description, signatories } = req.body;

    if (!amount || !receiver || !proposalId || !description || !signatories) {
      return res.status(400).json({ error: 'Missing required fields' });
    }

    await treasury.withdraw(amount, receiver, proposalId, description, signatories);

    res.json({ success: true });
  } catch (error) {
    res.status(500).json({ error: error.message });
  }
};
```

## Implementation Steps

To implement these features, follow these steps:

1. Create the basic data models and storage classes:

   - Create the `src/governance` directory
   - Implement models.rs, storage.rs, and treasury.rs
   - Add the module structure in mod.rs

2. Implement the governance manager:

   - Create the GovernanceManager class as defined above
   - Ensure proper error handling and validation

3. Add the API endpoints:

   - Create the governance controller and routes
   - Connect the API to the core governance functionality

4. Implement the front-end components:

   - Create the proposal listing page
   - Create the proposal detail and voting page
   - Add navigation and integration with the API

5. Test the implementation:
   - Test proposal creation
   - Test voting functionality
   - Test treasury operations

This implementation provides the foundation for our governance system with the most impactful
low-hanging fruit. It includes:

1. A complete proposal system with creation, voting, and execution
2. Treasury management with multi-signature security
3. A user-friendly interface for governance participation

As we progress through the phases in our plan, we can build on this foundation to add more advanced
features like conviction voting, specialized committees, and adaptive quorum biasing.
