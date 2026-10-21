//! Raft consensus algorithm implementation
//!
//! Adapted and merged from:
//! - `blockchain-core/src/consensus/raft_consensus_manager.rs`
//! - `blockchain-core/src/consensus/raft_consensus_factory.rs`
//! - `blockchain-core/src/consensus/raft_integration_coordinator.rs`
//!
//! Implements a production-ready Raft consensus algorithm for distributed systems
//! with strong consistency guarantees. Provides leader election, log replication,
//! and cluster membership management.
//!
//! The `ByzantineConsensusCoordinator` integration from blockchain-core has been
//! removed since it references deeply-tangled blockchain-core internals.

use async_trait::async_trait;
use log::{debug, info};
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use rustc_hash::FxHashMap;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::{RwLock as TokioRwLock, broadcast, mpsc};

use wrfcoin_shared_protocol::blockchain::block::Block;
use wrfcoin_shared_protocol::blockchain::chain::Blockchain;
use wrfcoin_shared_protocol::blockchain::transaction::Transaction;
use wrfcoin_shared_protocol::crypto::hash::hash_bytes;

use crate::traits::ConsensusEngine;
use crate::types::{ConsensusConfig, ConsensusError, ConsensusResult};

// ---------------------------------------------------------------------------
// Raft types
// ---------------------------------------------------------------------------

/// Raft node state
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RaftState {
    /// Follower state - receives logs from leader
    Follower,
    /// Candidate state - requests votes for leadership
    Candidate,
    /// Leader state - coordinates cluster and replicates logs
    Leader,
}

/// Raft log entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    /// Entry index in the log
    pub index: u64,
    /// Term when entry was created
    pub term: u64,
    /// Entry command/data
    pub command: Vec<u8>,
    /// Entry timestamp
    pub timestamp: u64,
    /// Entry type for consensus coordination
    pub entry_type: LogEntryType,
    /// Cryptographic hash for integrity
    pub hash: String,
    /// Reference to previous entry for chain integrity
    pub prev_hash: String,
}

/// Types of log entries for different consensus operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LogEntryType {
    /// Standard consensus operation
    ConsensusOperation,
    /// Leader election coordination
    LeaderElection,
    /// Cluster membership change
    MembershipChange,
    /// Configuration update
    Configuration,
    /// Cross-chain coordination
    CrossChain,
    /// Governance decision
    Governance,
    /// Emergency operation
    Emergency,
}

/// Raft consensus message types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RaftMessage {
    /// Request votes for leader election
    RequestVote {
        term: u64,
        candidate_id: String,
        last_log_index: u64,
        last_log_term: u64,
        election_id: String,
    },
    /// Response to vote request
    VoteResponse {
        term: u64,
        vote_granted: bool,
        voter_id: String,
        election_id: String,
    },
    /// Append entries (heartbeat and log replication)
    AppendEntries {
        term: u64,
        leader_id: String,
        prev_log_index: u64,
        prev_log_term: u64,
        entries: Vec<LogEntry>,
        leader_commit: u64,
    },
    /// Response to append entries
    AppendEntriesResponse {
        term: u64,
        success: bool,
        follower_id: String,
        match_index: u64,
        next_index: u64,
    },
    /// Install snapshot message
    InstallSnapshot {
        term: u64,
        leader_id: String,
        last_included_index: u64,
        last_included_term: u64,
        data: Vec<u8>,
    },
}

/// Raft configuration parameters
#[derive(Debug, Clone)]
pub struct RaftConfig {
    /// Minimum election timeout (ms)
    pub election_timeout_min: u64,
    /// Maximum election timeout (ms)
    pub election_timeout_max: u64,
    /// Heartbeat interval (ms)
    pub heartbeat_interval: u64,
    /// Log compaction threshold (entries)
    pub log_compaction_threshold: u64,
    /// Maximum log entries per append request
    pub max_append_entries: usize,
    /// Cluster membership configuration
    pub cluster_config: ClusterConfig,
    /// Performance monitoring enabled
    pub performance_monitoring: bool,
    /// Snapshot creation threshold
    pub snapshot_threshold: u64,
}

impl Default for RaftConfig {
    fn default() -> Self {
        Self {
            election_timeout_min: 150,
            election_timeout_max: 300,
            heartbeat_interval: 50,
            log_compaction_threshold: 1000,
            max_append_entries: 100,
            cluster_config: ClusterConfig::default(),
            performance_monitoring: true,
            snapshot_threshold: 500,
        }
    }
}

/// Cluster membership configuration
#[derive(Debug, Clone)]
pub struct ClusterConfig {
    /// Minimum cluster size for consensus
    pub min_cluster_size: usize,
    /// Maximum cluster size
    pub max_cluster_size: usize,
    /// Dynamic membership changes allowed
    pub dynamic_membership: bool,
    /// Joint consensus for membership changes
    pub joint_consensus: bool,
}

impl Default for ClusterConfig {
    fn default() -> Self {
        Self {
            min_cluster_size: 3,
            max_cluster_size: 21,
            dynamic_membership: true,
            joint_consensus: true,
        }
    }
}

/// Cluster member information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterMember {
    /// Member ID
    pub id: String,
    /// Member endpoint address
    pub address: String,
    /// Member status
    pub status: MemberStatus,
    /// Last heartbeat received
    pub last_heartbeat: u64,
    /// Member voting weight
    pub voting_weight: u64,
    /// Member reputation score
    pub reputation: f64,
    /// Join timestamp
    pub joined_at: u64,
}

/// Cluster member status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum MemberStatus {
    /// Active voting member
    Active,
    /// Staging member (joining)
    Staging,
    /// Leaving member
    Leaving,
    /// Failed/unreachable member
    Failed,
    /// Removed from cluster
    Removed,
}

/// Raft performance metrics
#[derive(Debug, Clone)]
pub struct RaftMetrics {
    /// Number of successful elections
    pub elections_won: u64,
    /// Number of failed elections
    pub elections_lost: u64,
    /// Total log entries processed
    pub log_entries_processed: u64,
    /// Average replication latency (ms)
    pub avg_replication_latency: f64,
    /// Heartbeat success rate
    pub heartbeat_success_rate: f64,
    /// Cluster membership changes
    pub membership_changes: u64,
    /// Leadership tenure average (ms)
    pub avg_leadership_tenure: f64,
}

impl Default for RaftMetrics {
    fn default() -> Self {
        Self {
            elections_won: 0,
            elections_lost: 0,
            log_entries_processed: 0,
            avg_replication_latency: 0.0,
            heartbeat_success_rate: 1.0,
            membership_changes: 0,
            avg_leadership_tenure: 0.0,
        }
    }
}

/// Raft snapshot for log compaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaftSnapshot {
    /// Last included log index
    pub last_included_index: u64,
    /// Last included log term
    pub last_included_term: u64,
    /// State machine snapshot data
    pub data: Vec<u8>,
    /// Cluster membership at snapshot time
    pub membership: Vec<ClusterMember>,
    /// Snapshot creation timestamp
    pub created_at: u64,
    /// Snapshot hash for integrity
    pub hash: String,
}

// ---------------------------------------------------------------------------
// RaftConsensusManager
// ---------------------------------------------------------------------------

/// Main Raft consensus manager
#[derive(Debug)]
#[allow(dead_code)]
pub struct RaftConsensusManager {
    /// Node ID
    node_id: String,
    /// Current state
    state: Arc<RwLock<RaftState>>,
    /// Current term
    current_term: Arc<RwLock<u64>>,
    /// Voted for in current term
    voted_for: Arc<RwLock<Option<String>>>,
    /// Log entries
    log: Arc<RwLock<Vec<LogEntry>>>,
    /// Index of highest log entry known to be committed
    commit_index: Arc<RwLock<u64>>,
    /// Index of highest log entry applied to state machine
    last_applied: Arc<RwLock<u64>>,
    /// For leaders: next index to send to each server
    next_index: Arc<RwLock<FxHashMap<String, u64>>>,
    /// For leaders: highest log entry index known to be replicated
    match_index: Arc<RwLock<FxHashMap<String, u64>>>,
    /// Configuration
    config: RaftConfig,
    /// Cluster membership
    cluster_members: Arc<TokioRwLock<FxHashMap<String, ClusterMember>>>,
    /// Message channel for Raft communications
    message_tx: broadcast::Sender<RaftMessage>,
    /// Command channel for external operations
    command_tx: mpsc::Sender<LogEntry>,
    command_rx: Arc<Mutex<Option<mpsc::Receiver<LogEntry>>>>,
    /// Current leader
    current_leader: Arc<RwLock<Option<String>>>,
    /// Election timeout
    election_timeout: Arc<RwLock<Duration>>,
    /// Last heartbeat received
    last_heartbeat: Arc<RwLock<Instant>>,
    /// Performance metrics
    metrics: Arc<RwLock<RaftMetrics>>,
    /// Current snapshot
    snapshot: Arc<RwLock<Option<RaftSnapshot>>>,
    /// Running state
    is_running: Arc<RwLock<bool>>,
}

#[allow(dead_code)]
impl RaftConsensusManager {
    /// Create a new Raft consensus manager
    pub async fn new(node_id: String, config: RaftConfig) -> ConsensusResult<Self> {
        info!("Initializing Raft Consensus Manager for node: {node_id}");

        let (message_tx, _) = broadcast::channel(1000);
        let (command_tx, command_rx) = mpsc::channel(100);

        let mut cluster_members = FxHashMap::default();

        cluster_members.insert(
            node_id.clone(),
            ClusterMember {
                id: node_id.clone(),
                address: "localhost:0".to_string(),
                status: MemberStatus::Active,
                last_heartbeat: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as u64,
                voting_weight: 1,
                reputation: 1.0,
                joined_at: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
            },
        );

        let manager = Self {
            node_id: node_id.clone(),
            state: Arc::new(RwLock::new(RaftState::Follower)),
            current_term: Arc::new(RwLock::new(0)),
            voted_for: Arc::new(RwLock::new(None)),
            log: Arc::new(RwLock::new(Vec::new())),
            commit_index: Arc::new(RwLock::new(0)),
            last_applied: Arc::new(RwLock::new(0)),
            next_index: Arc::new(RwLock::new(FxHashMap::default())),
            match_index: Arc::new(RwLock::new(FxHashMap::default())),
            config,
            cluster_members: Arc::new(TokioRwLock::new(cluster_members)),
            message_tx,
            command_tx,
            command_rx: Arc::new(Mutex::new(Some(command_rx))),
            current_leader: Arc::new(RwLock::new(None)),
            election_timeout: Arc::new(RwLock::new(Duration::from_millis(150))),
            last_heartbeat: Arc::new(RwLock::new(Instant::now())),
            metrics: Arc::new(RwLock::new(RaftMetrics::default())),
            snapshot: Arc::new(RwLock::new(None)),
            is_running: Arc::new(RwLock::new(false)),
        };

        info!("Raft Consensus Manager initialized successfully for node: {node_id}");
        Ok(manager)
    }

    /// Start the Raft consensus manager
    pub async fn start(&self) -> ConsensusResult<()> {
        info!("Starting Raft consensus manager for node: {}", self.node_id);
        *self.is_running.write().unwrap() = true;
        info!("Raft consensus manager started successfully");
        Ok(())
    }

    /// Stop the Raft consensus manager
    pub async fn stop(&self) -> ConsensusResult<()> {
        info!("Stopping Raft consensus manager for node: {}", self.node_id);
        *self.is_running.write().unwrap() = false;
        Ok(())
    }

    /// Submit a command to the Raft cluster
    #[allow(clippy::await_holding_lock)]
    pub async fn submit_command(
        &self,
        command: Vec<u8>,
        entry_type: LogEntryType,
    ) -> ConsensusResult<u64> {
        let state = self.state.read().unwrap().clone();
        if state != RaftState::Leader {
            let leader = self.current_leader.read().unwrap().clone();
            return Err(ConsensusError::InvalidState(format!(
                "Not leader, current leader: {leader:?}"
            )));
        }

        let term = *self.current_term.read().unwrap();
        let log = self.log.read().unwrap();
        let index = log.len() as u64 + 1;
        let prev_hash = log.last().map(|e| e.hash.clone()).unwrap_or_default();
        drop(log);

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        let entry = LogEntry {
            index,
            term,
            command,
            timestamp,
            entry_type,
            hash: self.calculate_entry_hash(index, term, &[], timestamp),
            prev_hash,
        };

        self.command_tx
            .send(entry.clone())
            .await
            .map_err(|e| ConsensusError::Other(format!("Failed to submit command: {e}")))?;

        Ok(index)
    }

    /// Add a new member to the cluster
    pub async fn add_cluster_member(&self, member: ClusterMember) -> ConsensusResult<()> {
        if !self.config.cluster_config.dynamic_membership {
            return Err(ConsensusError::ConfigurationError(
                "Dynamic membership not enabled".to_string(),
            ));
        }

        let state = self.state.read().unwrap().clone();
        if state != RaftState::Leader {
            return Err(ConsensusError::InvalidState(
                "Only leader can add members".to_string(),
            ));
        }

        let mut members = self.cluster_members.write().await;
        if members.len() >= self.config.cluster_config.max_cluster_size {
            return Err(ConsensusError::ConfigurationError(
                "Cluster at maximum size".to_string(),
            ));
        }

        info!("Adding new cluster member: {}", member.id);
        members.insert(member.id.clone(), member);

        let mut metrics = self.metrics.write().unwrap();
        metrics.membership_changes += 1;

        Ok(())
    }

    /// Remove a member from the cluster
    pub async fn remove_cluster_member(&self, member_id: &str) -> ConsensusResult<()> {
        if !self.config.cluster_config.dynamic_membership {
            return Err(ConsensusError::ConfigurationError(
                "Dynamic membership not enabled".to_string(),
            ));
        }

        let state = self.state.read().unwrap().clone();
        if state != RaftState::Leader {
            return Err(ConsensusError::InvalidState(
                "Only leader can remove members".to_string(),
            ));
        }

        let mut members = self.cluster_members.write().await;
        if members.len() <= self.config.cluster_config.min_cluster_size {
            return Err(ConsensusError::ConfigurationError(
                "Cluster at minimum size".to_string(),
            ));
        }

        info!("Removing cluster member: {member_id}");
        if let Some(member) = members.get_mut(member_id) {
            member.status = MemberStatus::Leaving;
        }

        let mut metrics = self.metrics.write().unwrap();
        metrics.membership_changes += 1;

        Ok(())
    }

    /// Get current cluster members
    pub async fn get_cluster_members(&self) -> FxHashMap<String, ClusterMember> {
        self.cluster_members.read().await.clone()
    }

    /// Get current leader
    pub fn get_current_leader(&self) -> Option<String> {
        self.current_leader.read().unwrap().clone()
    }

    /// Get current state
    pub fn get_state(&self) -> RaftState {
        self.state.read().unwrap().clone()
    }

    /// Get current term
    pub fn get_current_term(&self) -> u64 {
        *self.current_term.read().unwrap()
    }

    /// Get performance metrics
    pub fn get_raft_metrics(&self) -> RaftMetrics {
        self.metrics.read().unwrap().clone()
    }

    /// Force leader election (for testing/emergency)
    pub async fn force_election(&self) -> ConsensusResult<()> {
        info!("Forcing leader election for node: {}", self.node_id);
        self.become_candidate().await?;
        self.start_election().await
    }

    /// Handle incoming Raft messages
    async fn handle_message(&self, message: RaftMessage) -> ConsensusResult<()> {
        match message {
            RaftMessage::RequestVote {
                term,
                candidate_id,
                last_log_index,
                last_log_term,
                election_id,
            } => {
                self.handle_request_vote(
                    term,
                    candidate_id,
                    last_log_index,
                    last_log_term,
                    election_id,
                )
                .await
            }
            RaftMessage::VoteResponse {
                term,
                vote_granted,
                voter_id,
                election_id: _,
            } => {
                self.handle_vote_response(term, vote_granted, voter_id)
                    .await
            }
            RaftMessage::AppendEntries {
                term,
                leader_id,
                prev_log_index,
                prev_log_term,
                entries,
                leader_commit,
            } => {
                self.handle_append_entries(
                    term,
                    leader_id,
                    prev_log_index,
                    prev_log_term,
                    entries,
                    leader_commit,
                )
                .await
            }
            RaftMessage::AppendEntriesResponse {
                term,
                success,
                follower_id,
                match_index,
                next_index: _,
            } => {
                self.handle_append_entries_response(term, success, follower_id, match_index)
                    .await
            }
            RaftMessage::InstallSnapshot {
                term,
                leader_id,
                last_included_index,
                last_included_term,
                data,
            } => {
                self.handle_install_snapshot(
                    term,
                    leader_id,
                    last_included_index,
                    last_included_term,
                    data,
                )
                .await
            }
        }
    }

    async fn handle_request_vote(
        &self,
        term: u64,
        candidate_id: String,
        last_log_index: u64,
        last_log_term: u64,
        election_id: String,
    ) -> ConsensusResult<()> {
        let mut current_term = self.current_term.write().unwrap();
        let mut voted_for = self.voted_for.write().unwrap();

        if term > *current_term {
            *current_term = term;
            *voted_for = None;
            *self.state.write().unwrap() = RaftState::Follower;
        }

        let mut vote_granted = false;

        if term == *current_term
            && (voted_for.is_none() || voted_for.as_ref() == Some(&candidate_id))
        {
            let log = self.log.read().unwrap();
            let our_last_index = log.len() as u64;
            let our_last_term = log.last().map(|e| e.term).unwrap_or(0);

            if last_log_term > our_last_term
                || (last_log_term == our_last_term && last_log_index >= our_last_index)
            {
                vote_granted = true;
                *voted_for = Some(candidate_id.clone());
                self.reset_election_timeout();
            }
        }

        let response = RaftMessage::VoteResponse {
            term: *current_term,
            vote_granted,
            voter_id: self.node_id.clone(),
            election_id,
        };

        let _ = self.message_tx.send(response);

        debug!("Handled RequestVote from {candidate_id}: granted={vote_granted}");
        Ok(())
    }

    async fn handle_vote_response(
        &self,
        term: u64,
        vote_granted: bool,
        voter_id: String,
    ) -> ConsensusResult<()> {
        let current_term = *self.current_term.read().unwrap();
        let state = self.state.read().unwrap().clone();

        if term > current_term {
            *self.current_term.write().unwrap() = term;
            *self.state.write().unwrap() = RaftState::Follower;
            *self.voted_for.write().unwrap() = None;
            return Ok(());
        }

        if state == RaftState::Candidate && term == current_term && vote_granted {
            debug!("Received vote from {voter_id}");
            let members = self.cluster_members.read().await;
            let total_members = members.len();

            if total_members >= self.config.cluster_config.min_cluster_size {
                self.become_leader().await?;
            }
        }

        Ok(())
    }

    async fn handle_append_entries(
        &self,
        term: u64,
        leader_id: String,
        prev_log_index: u64,
        prev_log_term: u64,
        entries: Vec<LogEntry>,
        leader_commit: u64,
    ) -> ConsensusResult<()> {
        // DoS guard: a malicious or misconfigured leader could send an
        // AppendEntries message carrying millions of log entries, causing OOM
        // before any signature or term validation runs. Enforce the configured
        // per-message cap before touching any shared state.
        if entries.len() > self.config.max_append_entries {
            return Err(ConsensusError::ValidationFailed(format!(
                "AppendEntries from {leader_id} carries {} entries; \
                 max_append_entries={} (DoS guard)",
                entries.len(),
                self.config.max_append_entries,
            )));
        }

        let mut current_term = self.current_term.write().unwrap();
        let mut success = false;

        if term > *current_term {
            *current_term = term;
            *self.voted_for.write().unwrap() = None;
        }

        if term >= *current_term {
            *self.state.write().unwrap() = RaftState::Follower;
            *self.current_leader.write().unwrap() = Some(leader_id.clone());
            self.reset_election_timeout();

            let mut log = self.log.write().unwrap();

            if prev_log_index == 0
                || (prev_log_index <= log.len() as u64
                    && log
                        .get((prev_log_index - 1) as usize)
                        .map(|e| e.term)
                        == Some(prev_log_term))
            {
                success = true;

                if prev_log_index < log.len() as u64 {
                    log.truncate(prev_log_index as usize);
                }

                for entry in entries {
                    log.push(entry);
                }

                if leader_commit > *self.commit_index.read().unwrap() {
                    let new_commit = std::cmp::min(leader_commit, log.len() as u64);
                    *self.commit_index.write().unwrap() = new_commit;
                }
            }
        }

        let response = RaftMessage::AppendEntriesResponse {
            term: *current_term,
            success,
            follower_id: self.node_id.clone(),
            match_index: if success {
                self.log.read().unwrap().len() as u64
            } else {
                0
            },
            next_index: self.log.read().unwrap().len() as u64 + 1,
        };

        let _ = self.message_tx.send(response);

        debug!("Handled AppendEntries from {leader_id}: success={success}");
        Ok(())
    }

    async fn handle_append_entries_response(
        &self,
        term: u64,
        success: bool,
        follower_id: String,
        match_index: u64,
    ) -> ConsensusResult<()> {
        let current_term = *self.current_term.read().unwrap();

        if term > current_term {
            *self.current_term.write().unwrap() = term;
            *self.state.write().unwrap() = RaftState::Follower;
            *self.voted_for.write().unwrap() = None;
            return Ok(());
        }

        let state = self.state.read().unwrap().clone();
        if state == RaftState::Leader && term == current_term {
            if success {
                let mut match_idx = self.match_index.write().unwrap();
                let mut next_idx = self.next_index.write().unwrap();

                match_idx.insert(follower_id.clone(), match_index);
                next_idx.insert(follower_id, match_index + 1);
            } else {
                let mut next_idx = self.next_index.write().unwrap();
                if let Some(next) = next_idx.get_mut(&follower_id) {
                    *next = (*next).saturating_sub(1);
                }
            }
        }

        Ok(())
    }

    async fn handle_install_snapshot(
        &self,
        term: u64,
        leader_id: String,
        last_included_index: u64,
        last_included_term: u64,
        data: Vec<u8>,
    ) -> ConsensusResult<()> {
        let current_term = *self.current_term.read().unwrap();

        if term >= current_term {
            *self.current_term.write().unwrap() = term;
            *self.state.write().unwrap() = RaftState::Follower;
            *self.current_leader.write().unwrap() = Some(leader_id);

            let snapshot = RaftSnapshot {
                last_included_index,
                last_included_term,
                data,
                membership: self
                    .cluster_members
                    .read()
                    .await
                    .values()
                    .cloned()
                    .collect(),
                created_at: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
                hash: self.calculate_snapshot_hash(last_included_index, last_included_term),
            };

            *self.snapshot.write().unwrap() = Some(snapshot);

            let mut log = self.log.write().unwrap();
            log.clear();

            *self.commit_index.write().unwrap() = last_included_index;
            *self.last_applied.write().unwrap() = last_included_index;

            info!("Installed snapshot from leader: index={last_included_index}");
        }

        Ok(())
    }

    /// Become candidate and start election
    async fn become_candidate(&self) -> ConsensusResult<()> {
        info!("Node {} becoming candidate", self.node_id);

        *self.state.write().unwrap() = RaftState::Candidate;
        let mut current_term = self.current_term.write().unwrap();
        *current_term += 1;
        *self.voted_for.write().unwrap() = Some(self.node_id.clone());

        self.reset_election_timeout();

        Ok(())
    }

    /// Start leader election
    #[allow(clippy::await_holding_lock)]
    async fn start_election(&self) -> ConsensusResult<()> {
        let term = *self.current_term.read().unwrap();
        let log = self.log.read().unwrap();
        let last_log_index = log.len() as u64;
        let last_log_term = log.last().map(|e| e.term).unwrap_or(0);
        drop(log);

        // Generate a simple election ID without uuid dependency
        let election_id = format!("election-{}-{}", self.node_id, term);

        info!("Starting election for term {term} (election: {election_id})");

        let members = self.cluster_members.read().await;
        for (member_id, member) in members.iter() {
            if member_id != &self.node_id && member.status == MemberStatus::Active {
                let request = RaftMessage::RequestVote {
                    term,
                    candidate_id: self.node_id.clone(),
                    last_log_index,
                    last_log_term,
                    election_id: election_id.clone(),
                };

                let _ = self.message_tx.send(request);
            }
        }

        Ok(())
    }

    /// Become leader
    async fn become_leader(&self) -> ConsensusResult<()> {
        info!(
            "Node {} becoming leader for term {}",
            self.node_id,
            *self.current_term.read().unwrap()
        );

        *self.state.write().unwrap() = RaftState::Leader;
        *self.current_leader.write().unwrap() = Some(self.node_id.clone());

        let log_len = self.log.read().unwrap().len() as u64;
        let members = self.cluster_members.read().await;

        let mut next_index = self.next_index.write().unwrap();
        let mut match_index = self.match_index.write().unwrap();

        for member_id in members.keys() {
            if member_id != &self.node_id {
                next_index.insert(member_id.clone(), log_len + 1);
                match_index.insert(member_id.clone(), 0);
            }
        }

        let mut metrics = self.metrics.write().unwrap();
        metrics.elections_won += 1;

        Ok(())
    }

    /// Check if election timeout has expired
    fn election_timeout_expired(&self) -> bool {
        let last_heartbeat = *self.last_heartbeat.read().unwrap();
        let timeout_duration = *self.election_timeout.read().unwrap();
        last_heartbeat.elapsed() > timeout_duration
    }

    /// Reset election timeout with randomization
    fn reset_election_timeout(&self) {
        let mut rng = rand::thread_rng();
        let timeout_ms =
            rng.gen_range(self.config.election_timeout_min..=self.config.election_timeout_max);

        *self.election_timeout.write().unwrap() = Duration::from_millis(timeout_ms);
        *self.last_heartbeat.write().unwrap() = Instant::now();
    }

    /// Calculate hash for log entry
    fn calculate_entry_hash(
        &self,
        index: u64,
        term: u64,
        command: &[u8],
        timestamp: u64,
    ) -> String {
        let mut hasher = Sha256::new();
        hasher.update(index.to_le_bytes());
        hasher.update(term.to_le_bytes());
        hasher.update(command);
        hasher.update(timestamp.to_le_bytes());
        format!("{:x}", hasher.finalize())
    }

    /// Calculate hash for snapshot
    fn calculate_snapshot_hash(&self, index: u64, term: u64) -> String {
        let mut hasher = Sha256::new();
        hasher.update(index.to_le_bytes());
        hasher.update(term.to_le_bytes());
        hasher.update(self.node_id.as_bytes());
        format!("{:x}", hasher.finalize())
    }
}

// ---------------------------------------------------------------------------
// RaftConsensusEngine - ConsensusEngine wrapper
// ---------------------------------------------------------------------------

/// Wrapper around `RaftConsensusManager` that implements the `ConsensusEngine` trait.
///
/// Bridges Raft's replicated log to the shared-protocol Block abstraction.
/// Blocks are created by the Raft leader and replicated to followers.
#[derive(Debug)]
pub struct RaftConsensusEngine {
    /// Internal Raft manager
    manager: Arc<TokioRwLock<RaftConsensusManager>>,
    /// Target block time
    target_block_time: u64,
    /// Algorithm configuration
    config: ConsensusConfig,
}

impl RaftConsensusEngine {
    /// Create a new Raft consensus engine
    pub async fn new(
        node_id: String,
        raft_config: RaftConfig,
        target_block_time: u64,
    ) -> ConsensusResult<Self> {
        let manager = RaftConsensusManager::new(node_id, raft_config).await?;
        Ok(Self {
            manager: Arc::new(TokioRwLock::new(manager)),
            target_block_time,
            config: ConsensusConfig::default(),
        })
    }

    /// Get a reference to the internal Raft manager.
    pub fn manager(&self) -> &Arc<TokioRwLock<RaftConsensusManager>> {
        &self.manager
    }
}

#[async_trait]
impl ConsensusEngine for RaftConsensusEngine {
    async fn validate_block(
        &self,
        block: &Block,
        _chain: &Blockchain,
    ) -> ConsensusResult<bool> {
        // Check consensus type
        if let Some(ct) = block.metadata.get("consensus_type") {
            if ct != "Raft" {
                return Ok(false);
            }
        }

        // Verify the block was produced by a known leader
        let producer = block
            .metadata
            .get("validator")
            .cloned()
            .unwrap_or_default();

        let manager = self.manager.read().await;
        let members = manager.get_cluster_members().await;

        // Block producer must be a cluster member
        if !members.contains_key(&producer) {
            return Ok(false);
        }

        Ok(block.hash.is_some())
    }

    async fn create_block(
        &self,
        transactions: Vec<Transaction>,
        previous_hash: &str,
        producer: &str,
        chain: &Blockchain,
    ) -> ConsensusResult<Block> {
        // Verify this node is the Raft leader
        {
            let manager = self.manager.read().await;
            if manager.get_state() != RaftState::Leader {
                return Err(ConsensusError::CreationFailed(
                    "Only the Raft leader can create blocks".to_string(),
                ));
            }
        }

        let prev = hash_bytes(previous_hash.as_bytes());

        let mut block = Block::new(
            3, // version
            prev,
            transactions,
            1, // Raft doesn't use difficulty
            chain.height + 1,
        );

        block
            .metadata
            .insert("validator".into(), producer.into());
        block
            .metadata
            .insert("consensus_type".into(), "Raft".into());

        let hash = block.calculate_hash();
        block.set_hash(hash);

        Ok(block)
    }

    async fn get_difficulty(&self) -> u64 {
        1
    }

    fn algorithm_name(&self) -> &'static str {
        "Raft"
    }

    async fn configure(&mut self, config: &ConsensusConfig) -> ConsensusResult<()> {
        self.config = config.clone();
        self.target_block_time = config.target_block_time;
        Ok(())
    }

    fn get_metrics(&self) -> FxHashMap<String, serde_json::Value> {
        let mut metrics = FxHashMap::default();
        metrics.insert("algorithm".to_string(), "Raft".into());
        metrics.insert(
            "target_block_time".to_string(),
            self.target_block_time.into(),
        );
        metrics
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_member(id: &str) -> ClusterMember {
        ClusterMember {
            id: id.to_string(),
            address: format!("localhost:{}", 8000 + id.len()),
            status: MemberStatus::Active,
            last_heartbeat: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
            voting_weight: 1,
            reputation: 1.0,
            joined_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        }
    }

    // --- Existing tests (kept) ---

    #[tokio::test]
    async fn test_raft_creation() {
        let config = RaftConfig::default();
        let manager = RaftConsensusManager::new("node1".to_string(), config).await;

        assert!(manager.is_ok());
        let manager = manager.unwrap();
        assert_eq!(manager.get_state(), RaftState::Follower);
        assert_eq!(manager.get_current_term(), 0);
    }

    #[tokio::test]
    async fn test_force_election() {
        let config = RaftConfig::default();
        let manager = RaftConsensusManager::new("node1".to_string(), config)
            .await
            .unwrap();

        let result = manager.force_election().await;
        assert!(result.is_ok());
        assert_eq!(manager.get_state(), RaftState::Candidate);
    }

    #[tokio::test]
    async fn test_cluster_membership() {
        let config = RaftConfig::default();
        let manager = RaftConsensusManager::new("node1".to_string(), config)
            .await
            .unwrap();

        // First become leader to add members
        manager.force_election().await.unwrap();
        *manager.state.write().unwrap() = RaftState::Leader;

        let result = manager.add_cluster_member(make_member("node2")).await;
        assert!(result.is_ok());

        let members = manager.get_cluster_members().await;
        assert!(members.contains_key("node2"));
    }

    // --- New tests ---

    #[tokio::test]
    async fn test_term_advances_on_election() {
        let manager = RaftConsensusManager::new("node1".to_string(), RaftConfig::default())
            .await
            .unwrap();

        assert_eq!(manager.get_current_term(), 0);
        manager.force_election().await.unwrap();
        assert_eq!(manager.get_current_term(), 1);

        // Second election increments again
        manager.force_election().await.unwrap();
        assert_eq!(manager.get_current_term(), 2);
    }

    #[tokio::test]
    async fn test_voted_for_self_on_candidacy() {
        let manager = RaftConsensusManager::new("node1".to_string(), RaftConfig::default())
            .await
            .unwrap();

        assert!(manager.voted_for.read().unwrap().is_none());
        manager.force_election().await.unwrap();
        assert_eq!(
            *manager.voted_for.read().unwrap(),
            Some("node1".to_string())
        );
    }

    #[tokio::test]
    async fn test_handle_request_vote_grants_vote() {
        let manager = RaftConsensusManager::new("follower1".to_string(), RaftConfig::default())
            .await
            .unwrap();

        // Subscribe to outgoing messages before sending request
        let mut rx = manager.message_tx.subscribe();

        let msg = RaftMessage::RequestVote {
            term: 5,
            candidate_id: "candidate1".to_string(),
            last_log_index: 0,
            last_log_term: 0,
            election_id: "election-test-5".to_string(),
        };
        manager.handle_message(msg).await.unwrap();

        // Term should be updated to the higher term
        assert_eq!(manager.get_current_term(), 5);
        // Should have voted for the candidate
        assert_eq!(
            *manager.voted_for.read().unwrap(),
            Some("candidate1".to_string())
        );

        // Check the outgoing VoteResponse
        let response = rx.recv().await.unwrap();
        match response {
            RaftMessage::VoteResponse {
                term,
                vote_granted,
                voter_id,
                ..
            } => {
                assert_eq!(term, 5);
                assert!(vote_granted);
                assert_eq!(voter_id, "follower1");
            }
            other => panic!("Expected VoteResponse, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_handle_request_vote_rejects_stale_term() {
        let manager = RaftConsensusManager::new("follower1".to_string(), RaftConfig::default())
            .await
            .unwrap();

        // Set current term to 5
        *manager.current_term.write().unwrap() = 5;

        let mut rx = manager.message_tx.subscribe();

        let msg = RaftMessage::RequestVote {
            term: 3, // stale term
            candidate_id: "old_candidate".to_string(),
            last_log_index: 0,
            last_log_term: 0,
            election_id: "election-stale".to_string(),
        };
        manager.handle_message(msg).await.unwrap();

        // Term should not change
        assert_eq!(manager.get_current_term(), 5);

        // Vote should not be granted
        let response = rx.recv().await.unwrap();
        match response {
            RaftMessage::VoteResponse {
                vote_granted,
                term,
                ..
            } => {
                assert!(!vote_granted);
                assert_eq!(term, 5);
            }
            other => panic!("Expected VoteResponse, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_handle_append_entries_resets_to_follower() {
        let manager = RaftConsensusManager::new("node1".to_string(), RaftConfig::default())
            .await
            .unwrap();

        // Make node a candidate first
        manager.force_election().await.unwrap();
        assert_eq!(manager.get_state(), RaftState::Candidate);

        let _rx = manager.message_tx.subscribe();

        // Receive AppendEntries from a leader with higher term
        let msg = RaftMessage::AppendEntries {
            term: 10,
            leader_id: "leader1".to_string(),
            prev_log_index: 0,
            prev_log_term: 0,
            entries: vec![],
            leader_commit: 0,
        };
        manager.handle_message(msg).await.unwrap();

        // Should step down to follower
        assert_eq!(manager.get_state(), RaftState::Follower);
        // Should recognize the leader
        assert_eq!(
            manager.get_current_leader(),
            Some("leader1".to_string())
        );
        // Term should be updated
        assert_eq!(manager.get_current_term(), 10);
    }

    #[tokio::test]
    async fn test_handle_append_entries_appends_log() {
        let manager = RaftConsensusManager::new("follower1".to_string(), RaftConfig::default())
            .await
            .unwrap();

        let _rx = manager.message_tx.subscribe();

        let entry = LogEntry {
            index: 1,
            term: 1,
            command: b"tx-data".to_vec(),
            timestamp: 1000,
            entry_type: LogEntryType::ConsensusOperation,
            hash: "abc123".to_string(),
            prev_hash: String::new(),
        };

        let msg = RaftMessage::AppendEntries {
            term: 1,
            leader_id: "leader1".to_string(),
            prev_log_index: 0,
            prev_log_term: 0,
            entries: vec![entry],
            leader_commit: 1,
        };
        manager.handle_message(msg).await.unwrap();

        // Log should contain the entry
        let log = manager.log.read().unwrap();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].term, 1);
        assert_eq!(log[0].command, b"tx-data");

        // Commit index should advance
        assert_eq!(*manager.commit_index.read().unwrap(), 1);
    }

    #[tokio::test]
    async fn test_submit_command_requires_leader() {
        let manager = RaftConsensusManager::new("node1".to_string(), RaftConfig::default())
            .await
            .unwrap();

        // Node starts as Follower — submit should fail
        let result = manager
            .submit_command(b"some-cmd".to_vec(), LogEntryType::ConsensusOperation)
            .await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, ConsensusError::InvalidState(_)));
    }

    // ── DoS guard: AppendEntries entries-vector bound ────────────────────────
    //
    // RaftConfig::max_append_entries caps the number of log entries per
    // AppendEntries message. Without enforcement a malicious or misconfigured
    // leader can send millions of entries and cause OOM before any term
    // validation runs.

    /// AppendEntries with more entries than max_append_entries must be rejected
    /// with a ValidationFailed error before modifying any log state.
    #[tokio::test]
    async fn test_append_entries_rejects_oversized_entries_vector() {
        let config = RaftConfig {
            max_append_entries: 3, // tighter limit for the test
            ..RaftConfig::default()
        };
        let manager = RaftConsensusManager::new("node1".to_string(), config)
            .await
            .unwrap();

        let _rx = manager.message_tx.subscribe();

        // Build 4 log entries — one over the limit.
        let make_entry = |idx: u64| LogEntry {
            index: idx,
            term: 1,
            command: format!("cmd-{idx}").into_bytes(),
            timestamp: 1000 + idx,
            entry_type: LogEntryType::ConsensusOperation,
            hash: format!("hash-{idx}"),
            prev_hash: if idx == 1 {
                String::new()
            } else {
                format!("hash-{}", idx - 1)
            },
        };

        let oversized: Vec<LogEntry> = (1..=4).map(make_entry).collect();

        let msg = RaftMessage::AppendEntries {
            term: 1,
            leader_id: "leader1".to_string(),
            prev_log_index: 0,
            prev_log_term: 0,
            entries: oversized,
            leader_commit: 0,
        };

        let result = manager.handle_message(msg).await;
        assert!(
            result.is_err(),
            "AppendEntries with entries.len()=4 > max_append_entries=3 must be rejected"
        );
        assert!(
            matches!(result.unwrap_err(), ConsensusError::ValidationFailed(_)),
            "expected ValidationFailed (DoS guard)"
        );

        // Log must remain empty — the guard fires before any state mutation.
        assert!(
            manager.log.read().unwrap().is_empty(),
            "log must not be modified when the DoS guard fires"
        );
    }

    /// AppendEntries with exactly max_append_entries entries must succeed.
    #[tokio::test]
    async fn test_append_entries_accepts_exactly_max_entries() {
        let config = RaftConfig {
            max_append_entries: 3,
            ..RaftConfig::default()
        };
        let manager = RaftConsensusManager::new("node1".to_string(), config)
            .await
            .unwrap();

        let _rx = manager.message_tx.subscribe();

        let make_entry = |idx: u64| LogEntry {
            index: idx,
            term: 1,
            command: format!("cmd-{idx}").into_bytes(),
            timestamp: 1000 + idx,
            entry_type: LogEntryType::ConsensusOperation,
            hash: format!("hash-{idx}"),
            prev_hash: if idx == 1 {
                String::new()
            } else {
                format!("hash-{}", idx - 1)
            },
        };

        // Exactly 3 entries — must not be rejected by the DoS guard.
        let entries: Vec<LogEntry> = (1..=3).map(make_entry).collect();

        let msg = RaftMessage::AppendEntries {
            term: 1,
            leader_id: "leader1".to_string(),
            prev_log_index: 0,
            prev_log_term: 0,
            entries,
            leader_commit: 3,
        };

        let result = manager.handle_message(msg).await;
        assert!(
            result.is_ok(),
            "AppendEntries at exactly max_append_entries must be accepted"
        );
        assert_eq!(
            manager.log.read().unwrap().len(),
            3,
            "all 3 entries must be appended"
        );
    }

    #[tokio::test]
    async fn test_handle_install_snapshot() {
        let manager = RaftConsensusManager::new("follower1".to_string(), RaftConfig::default())
            .await
            .unwrap();

        // Pre-populate log with an entry
        {
            let mut log = manager.log.write().unwrap();
            log.push(LogEntry {
                index: 1,
                term: 1,
                command: b"old".to_vec(),
                timestamp: 500,
                entry_type: LogEntryType::ConsensusOperation,
                hash: "old-hash".to_string(),
                prev_hash: String::new(),
            });
        }

        let snapshot_data = b"snapshot-state-machine-data".to_vec();
        let msg = RaftMessage::InstallSnapshot {
            term: 5,
            leader_id: "leader1".to_string(),
            last_included_index: 10,
            last_included_term: 4,
            data: snapshot_data.clone(),
        };
        manager.handle_message(msg).await.unwrap();

        // State should be follower, recognizing leader
        assert_eq!(manager.get_state(), RaftState::Follower);
        assert_eq!(manager.get_current_leader(), Some("leader1".to_string()));
        assert_eq!(manager.get_current_term(), 5);

        // Log should be cleared after snapshot install
        assert!(manager.log.read().unwrap().is_empty());

        // Commit and apply indices should match snapshot
        assert_eq!(*manager.commit_index.read().unwrap(), 10);
        assert_eq!(*manager.last_applied.read().unwrap(), 10);

        // Snapshot should be stored
        let snap = manager.snapshot.read().unwrap();
        let snap = snap.as_ref().expect("snapshot should be set");
        assert_eq!(snap.last_included_index, 10);
        assert_eq!(snap.last_included_term, 4);
        assert_eq!(snap.data, snapshot_data);
    }
}
