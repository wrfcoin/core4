#![allow(dead_code)]
#![allow(clippy::disallowed_types)]

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

use crate::blockchain::Blockchain;
use crate::nft::{ComputableNFT, NFTRegistry};

/// Represents the role or specialty of an AI agent
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AgentRole {
    /// Nova - Onboarding concierge
    Onboarding,
    /// Pulse - Outreach and marketing
    Marketing,
    /// Nimbus - Weather data interpreter
    Weather,
    /// Solace - Publishing and blog summarization
    Publishing,
    /// Sage - Research synthesizer
    Research,
    /// Echo - Governance assistant
    Governance,
    /// Orbit - Task generator
    TaskGeneration,
    /// Quanta - Tournament master
    Tournament,
}

impl AgentRole {
    /// Get the display name for this role
    pub fn display_name(&self) -> &'static str {
        match self {
            AgentRole::Onboarding => "Nova",
            AgentRole::Marketing => "Pulse",
            AgentRole::Weather => "Nimbus",
            AgentRole::Publishing => "Solace",
            AgentRole::Research => "Sage",
            AgentRole::Governance => "Echo",
            AgentRole::TaskGeneration => "Orbit",
            AgentRole::Tournament => "Quanta",
        }
    }

    /// Get the description for this role
    pub fn description(&self) -> &'static str {
        match self {
            AgentRole::Onboarding => {
                "Helps new users explore the Wrfcoin platform, connect wallets, wrap tokens, and understand capabilities."
            },
            AgentRole::Marketing => {
                "Manages Wrfcoin's social presence and community-building efforts."
            },
            AgentRole::Weather => {
                "Converts raw oracle/IOT weather data into helpful forecasts and decision-making insights."
            },
            AgentRole::Publishing => {
                "Crafts and posts long-form articles, op-eds, technical explainers, and blog updates."
            },
            AgentRole::Research => {
                "Turns raw outputs (model results, votes, forecasts) into understandable insights."
            },
            AgentRole::Governance => {
                "Helps users understand and participate in governance, even automating votes based on preference."
            },
            AgentRole::TaskGeneration => {
                "Generates and submits new analysis tasks based on observed data trends or gaps in market intelligence."
            },
            AgentRole::Tournament => {
                "Hosts seasonal forecasting competitions, rankings, and model battles."
            },
        }
    }

    /// Get a list of all available roles
    pub fn all_roles() -> Vec<AgentRole> {
        vec![
            AgentRole::Onboarding,
            AgentRole::Marketing,
            AgentRole::Weather,
            AgentRole::Publishing,
            AgentRole::Research,
            AgentRole::Governance,
            AgentRole::TaskGeneration,
            AgentRole::Tournament,
        ]
    }
}

/// Represents an AI agent in the system
#[derive(Debug, Clone)]
pub struct Agent {
    pub id: String,
    pub name: String,
    pub role: AgentRole,
    pub created_at: u64,
    pub reputation: u64,
    pub contributed_nfts: Vec<String>, // IDs of NFTs this agent contributed to
    pub capabilities: Vec<String>,
    pub last_active: u64,
    pub owner: Option<String>, // Optional owner address
}

impl Agent {
    /// Create a new agent
    pub fn new(
        name: String,
        role: AgentRole,
        capabilities: Vec<String>,
        owner: Option<String>,
    ) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs();

        // Generate an ID based on role and timestamp
        let id = format!("agent-{}-{}", role.display_name().to_lowercase(), timestamp);

        Agent {
            id,
            name,
            role,
            created_at: timestamp,
            reputation: 0,
            contributed_nfts: Vec::new(),
            capabilities,
            last_active: timestamp,
            owner,
        }
    }

    /// Update the agent's activity timestamp
    pub fn mark_active(&mut self) {
        self.last_active = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs();
    }

    /// Add a contributed NFT to this agent's portfolio
    pub fn add_contributed_nft(&mut self, nft_id: String) {
        if !self.contributed_nfts.contains(&nft_id) {
            self.contributed_nfts.push(nft_id);
        }
    }

    /// Increase the agent's reputation
    pub fn increase_reputation(&mut self, amount: u64) {
        self.reputation = self.reputation.saturating_add(amount);
    }

    /// Decrease the agent's reputation
    pub fn decrease_reputation(&mut self, amount: u64) {
        self.reputation = self.reputation.saturating_sub(amount);
    }
}

/// Represents a task analysis created by an agent
#[derive(Debug, Clone)]
pub struct AgentTask {
    pub id: String,
    pub title: String,
    pub description: String,
    pub agent_id: String,
    pub created_at: u64,
    pub status: AgentTaskStatus,
    pub task_type: AgentTaskType,
    pub importance: u8, // 1-10 scale
    pub deadline: Option<u64>,
    pub assigned_to: Option<String>, // ID of the agent assigned to complete this
    pub completed_at: Option<u64>,
    pub result_nft_id: Option<String>, // ID of the NFT created from this task's work
}

/// Status of an agent task
#[derive(Debug, Clone, PartialEq)]
pub enum AgentTaskStatus {
    Created,
    Assigned,
    InProgress,
    Completed,
    Failed,
    Expired,
}

/// Types of tasks that can be performed
#[derive(Debug, Clone)]
pub enum AgentTaskType {
    WeatherForecast,
    MarketAnalysis,
    ContentCreation,
    DataVisualization,
    GovernanceProposal,
    NetworkMonitoring,
    ReputationAssessment,
    TournamentCreation,
    Custom(String),
}

impl AgentTask {
    /// Create a new agent task
    pub fn new(
        title: String,
        description: String,
        agent_id: String,
        task_type: AgentTaskType,
        importance: u8,
        deadline: Option<u64>,
    ) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs();

        // Generate an ID based on agent and timestamp
        let id = format!("task-{agent_id}-{timestamp}");

        AgentTask {
            id,
            title,
            description,
            agent_id,
            created_at: timestamp,
            status: AgentTaskStatus::Created,
            task_type,
            importance: importance.min(10).max(1), // Ensure importance is 1-10
            deadline,
            assigned_to: None,
            completed_at: None,
            result_nft_id: None,
        }
    }

    /// Assign this task to an agent
    pub fn assign(&mut self, agent_id: String) {
        self.assigned_to = Some(agent_id);
        self.status = AgentTaskStatus::Assigned;
    }

    /// Mark this task as in progress
    pub fn start_progress(&mut self) {
        self.status = AgentTaskStatus::InProgress;
    }

    /// Mark this task as completed
    pub fn complete(&mut self, nft_id: Option<String>) {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs();

        self.status = AgentTaskStatus::Completed;
        self.completed_at = Some(timestamp);
        self.result_nft_id = nft_id;
    }

    /// Mark this task as failed
    pub fn fail(&mut self) {
        self.status = AgentTaskStatus::Failed;
    }

    /// Check if this task has expired
    pub fn check_expired(&mut self) -> bool {
        if let Some(deadline) = self.deadline {
            let current_time = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("Time went backwards")
                .as_secs();

            if current_time > deadline && self.status != AgentTaskStatus::Completed {
                self.status = AgentTaskStatus::Expired;
                return true;
            }
        }

        false
    }
}

/// The AgentRegistry manages and coordinates the AI agents
pub struct AgentRegistry {
    pub agents: HashMap<String, Agent>,
    pub tasks: HashMap<String, AgentTask>,
}

impl AgentRegistry {
    /// Create a new agent registry
    pub fn new() -> Self {
        AgentRegistry {
            agents: HashMap::new(),
            tasks: HashMap::new(),
        }
    }

    /// Add an agent to the registry
    pub fn add_agent(&mut self, agent: Agent) {
        self.agents.insert(agent.id.clone(), agent);
    }

    /// Get an agent by ID
    pub fn get_agent(&self, id: &str) -> Option<&Agent> {
        self.agents.get(id)
    }

    /// Get a mutable reference to an agent by ID
    pub fn get_agent_mut(&mut self, id: &str) -> Option<&mut Agent> {
        self.agents.get_mut(id)
    }

    /// Get all agents
    pub fn get_all_agents(&self) -> Vec<&Agent> {
        self.agents.values().collect()
    }

    /// Get agents by role
    pub fn get_agents_by_role(&self, role: &AgentRole) -> Vec<&Agent> {
        self.agents
            .values()
            .filter(|agent| agent.role == *role)
            .collect()
    }

    /// Add a task to the registry
    pub fn add_task(&mut self, task: AgentTask) {
        self.tasks.insert(task.id.clone(), task);
    }

    /// Get a task by ID
    pub fn get_task(&self, id: &str) -> Option<&AgentTask> {
        self.tasks.get(id)
    }

    /// Get a mutable reference to a task by ID
    pub fn get_task_mut(&mut self, id: &str) -> Option<&mut AgentTask> {
        self.tasks.get_mut(id)
    }

    /// Get all tasks
    pub fn get_all_tasks(&self) -> Vec<&AgentTask> {
        self.tasks.values().collect()
    }

    /// Get tasks created by an agent
    pub fn get_tasks_by_creator(&self, agent_id: &str) -> Vec<&AgentTask> {
        self.tasks
            .values()
            .filter(|task| task.agent_id == agent_id)
            .collect()
    }

    /// Get tasks assigned to an agent
    pub fn get_tasks_by_assignee(&self, agent_id: &str) -> Vec<&AgentTask> {
        self.tasks
            .values()
            .filter(|task| task.assigned_to == Some(agent_id.to_string()))
            .collect()
    }

    /// Get tasks by status
    pub fn get_tasks_by_status(&self, status: &AgentTaskStatus) -> Vec<&AgentTask> {
        self.tasks
            .values()
            .filter(|task| task.status == *status)
            .collect()
    }

    /// Create a new NFT using an agent
    #[allow(clippy::too_many_arguments)]
    pub fn create_agent_nft(
        &mut self,
        agent_id: &str,
        nft_registry: &mut NFTRegistry,
        title: String,
        description: String,
        input_data_hashes: Vec<String>,
        executable_code: String,
        outputs: String,
        metadata: HashMap<String, String>,
    ) -> Option<String> {
        // Get the agent
        let agent = match self.get_agent_mut(agent_id) {
            Some(agent) => agent,
            None => return None,
        };

        // Generate provenance vector including the agent
        let mut provenance = vec![
            format!("agent:{}", agent.id),
            format!("role:{}", agent.role.display_name().to_lowercase()),
        ];

        if let Some(owner) = &agent.owner {
            provenance.push(format!("owner:{owner}"));
        }

        // Create the NFT
        let nft = ComputableNFT::new(
            agent.id.clone(),
            title,
            description,
            input_data_hashes,
            executable_code,
            outputs,
            metadata,
            provenance,
        );

        let nft_id = nft.id.clone();

        // Add NFT to registry
        nft_registry.add_nft(nft);

        // Update agent with new contributed NFT
        agent.add_contributed_nft(nft_id.clone());
        agent.mark_active();
        agent.increase_reputation(1); // Basic reputation increase for creating an NFT

        Some(nft_id)
    }

    /// Assign a task to create an NFT
    #[allow(clippy::too_many_arguments)]
    pub fn assign_nft_creation_task(
        &mut self,
        creator_agent_id: &str,
        assigned_agent_id: &str,
        title: String,
        description: String,
        task_type: AgentTaskType,
        importance: u8,
        deadline: Option<u64>,
    ) -> Option<String> {
        // Verify both agents exist
        if !self.agents.contains_key(creator_agent_id)
            || !self.agents.contains_key(assigned_agent_id)
        {
            return None;
        }

        // Create the task
        let mut task = AgentTask::new(
            title,
            description,
            creator_agent_id.to_string(),
            task_type,
            importance,
            deadline,
        );

        // Assign it to the specified agent
        task.assign(assigned_agent_id.to_string());

        let task_id = task.id.clone();

        // Add to registry
        self.add_task(task);

        // Mark both agents as active
        if let Some(agent) = self.get_agent_mut(creator_agent_id) {
            agent.mark_active();
        }
        if let Some(agent) = self.get_agent_mut(assigned_agent_id) {
            agent.mark_active();
        }

        Some(task_id)
    }

    /// Complete a task by creating an NFT as the result
    #[allow(clippy::too_many_arguments)]
    pub async fn complete_task_with_nft(
        &mut self,
        task_id: &str,
        nft_registry: &mut NFTRegistry,
        blockchain: Arc<RwLock<Blockchain>>,
        input_data_hashes: Vec<String>,
        executable_code: String,
        outputs: String,
        additional_metadata: HashMap<String, String>,
    ) -> Option<String> {
        // Extract all the information we need before modifying anything
        let (
            task_title,
            task_description,
            task_importance,
            task_agent_id,
            task_type,
            assigned_agent_id,
        ) = {
            let task = match self.get_task(task_id) {
                Some(task) => task,
                None => return None,
            };

            // Check if the task is assigned and not yet completed
            if task.status != AgentTaskStatus::Assigned
                && task.status != AgentTaskStatus::InProgress
            {
                return None;
            }

            // Get the assigned agent id
            let assigned_id = match &task.assigned_to {
                Some(id) => id.clone(),
                None => return None,
            };

            (
                task.title.clone(),
                task.description.clone(),
                task.importance,
                task.agent_id.clone(),
                format!("{:?}", task.task_type),
                assigned_id,
            )
        };

        // Get agent information
        let agent_info = {
            let agent = match self.get_agent(&assigned_agent_id) {
                Some(agent) => agent,
                None => return None,
            };

            (agent.id.clone(), agent.role.display_name().to_lowercase())
        };

        // Prepare NFT metadata
        let mut metadata = additional_metadata;
        metadata.insert("task_id".to_string(), task_id.to_string());
        metadata.insert("task_type".to_string(), task_type);
        metadata.insert("creator_agent".to_string(), task_agent_id.clone());
        metadata.insert("importance".to_string(), task_importance.to_string());

        // Create the NFT
        let nft = ComputableNFT::new(
            agent_info.0.clone(),
            task_title,
            task_description,
            input_data_hashes,
            executable_code,
            outputs,
            metadata,
            vec![
                format!("agent:{}", agent_info.0),
                format!("role:{}", agent_info.1),
                format!("task:{task_id}"),
            ],
        );

        let nft_id = nft.id.clone();

        // Add NFT to registry
        nft_registry.add_nft(nft.clone());

        // Create blockchain transaction for minting
        {
            let mut chain = blockchain.write().await;
            chain.add_mint_nft_transaction(&agent_info.0, &nft_id);
            // Mine the NFT transaction
            chain.mine_pending_transactions(&agent_info.0);
        }

        // Now we can start modifying things

        // Update assigned agent stats
        {
            let agent = self.get_agent_mut(&assigned_agent_id).unwrap();
            agent.add_contributed_nft(nft_id.clone());
            agent.mark_active();
            agent.increase_reputation(task_importance as u64); // Reputation increase based on task importance
        }

        // Mark task as completed
        {
            let task = self.get_task_mut(task_id).unwrap();
            task.complete(Some(nft_id.clone()));
        }

        // Also update the task creator's reputation
        if task_agent_id != assigned_agent_id {
            // Only if creator is different from assigned agent
            if let Some(creator_agent) = self.get_agent_mut(&task_agent_id) {
                creator_agent.increase_reputation(1); // Small reputation increase for task creator
            }
        }

        Some(nft_id)
    }
}
