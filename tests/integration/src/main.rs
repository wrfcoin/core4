#![allow(clippy::disallowed_types)]

mod agents;
mod blockchain;
mod nft;
mod nodes;
mod resources;
mod templates;

use agents::{Agent, AgentRegistry, AgentRole, AgentTask, AgentTaskStatus, AgentTaskType};
use blockchain::{BlockType, Blockchain};
use nft::{ComputableNFT, EngagementMetrics, NFTRegistry, VerificationStatus};
use nodes::NodeRegistry;
use resources::{ComputationPriority, ComputationScheduler, ResourceManager};
use std::collections::HashMap;
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::RwLock;

/// Representation of the server state
struct ServerState {
    blockchain: Blockchain,
    nft_registry: NFTRegistry,
    agent_registry: AgentRegistry,
    node_registry: NodeRegistry,
    resource_manager: Arc<ResourceManager>,
    computation_scheduler: Arc<RwLock<ComputationScheduler>>,
}

#[tokio::main]
async fn main() -> io::Result<()> {
    // Initialize the resource manager
    let resource_manager = Arc::new(ResourceManager::new());

    // Initialize the computation scheduler
    let computation_scheduler = Arc::new(RwLock::new(ComputationScheduler::new(
        resource_manager.clone(),
    )));

    // Initialize the server state
    let state = Arc::new(RwLock::new(ServerState {
        blockchain: Blockchain::new(),
        nft_registry: NFTRegistry::new(),
        agent_registry: AgentRegistry::new(),
        node_registry: NodeRegistry::new(),
        resource_manager: resource_manager.clone(),
        computation_scheduler: computation_scheduler.clone(),
    }));

    // Create some example addresses and seed them with coins
    {
        let mut server_state = state.write().await;
        server_state
            .blockchain
            .balances
            .insert("alice".to_string(), 100);
        server_state
            .blockchain
            .balances
            .insert("bob".to_string(), 50);
        server_state
            .blockchain
            .balances
            .insert("validator".to_string(), 30);

        // Add a few transactions to demonstrate functionality
        server_state
            .blockchain
            .add_transfer_transaction("alice", "bob", 10);
        server_state
            .blockchain
            .add_transfer_transaction("bob", "alice", 5);

        // Mine the transactions
        server_state.blockchain.mine_pending_transactions("miner");

        // Create AI agents for various roles and store their IDs
        let nova = Agent::new(
            "Nova".to_string(),
            AgentRole::Onboarding,
            vec![
                "help".to_string(),
                "explain".to_string(),
                "guide".to_string(),
            ],
            None,
        );
        // Store ID for potential future use
        let _nova_id = nova.id.clone();
        server_state.agent_registry.add_agent(nova);

        let nimbus = Agent::new(
            "Nimbus".to_string(),
            AgentRole::Weather,
            vec![
                "forecast".to_string(),
                "analyze".to_string(),
                "predict".to_string(),
            ],
            Some("alice".to_string()),
        );
        let nimbus_id = nimbus.id.clone();
        server_state.agent_registry.add_agent(nimbus);

        let pulse = Agent::new(
            "Pulse".to_string(),
            AgentRole::Marketing,
            vec![
                "promote".to_string(),
                "share".to_string(),
                "engage".to_string(),
            ],
            Some("bob".to_string()),
        );
        // Store ID for potential future use
        let _pulse_id = pulse.id.clone();
        server_state.agent_registry.add_agent(pulse);

        let sage = Agent::new(
            "Sage".to_string(),
            AgentRole::Research,
            vec![
                "synthesize".to_string(),
                "summarize".to_string(),
                "analyze".to_string(),
            ],
            None,
        );
        let sage_id = sage.id.clone();
        server_state.agent_registry.add_agent(sage);

        // Create a sample computable NFT
        let mut metadata = HashMap::new();
        metadata.insert("task_type".to_string(), "weather_forecast".to_string());
        metadata.insert("location".to_string(), "New York".to_string());

        let input_hashes =
            vec!["ipfs://QmXoypizjW3WknFiJnKLwHCnL72vedxjQkDDP1mXWo6uco".to_string()];

        let nft = ComputableNFT::new(
            "alice".to_string(),
            "NYC Weather Forecast".to_string(),
            "A 7-day weather forecast for New York City".to_string(),
            input_hashes,
            "import pandas as pd\nimport matplotlib.pyplot as plt\n# Analysis code here"
                .to_string(),
            "Weather will be sunny with highs of 75F".to_string(),
            metadata,
            vec![
                "weather_api".to_string(),
                "machine_learning_model".to_string(),
            ],
        );

        // Add NFT to registry
        let nft_id = nft.id.clone();
        server_state.nft_registry.add_nft(nft);

        // Create mint transaction
        server_state
            .blockchain
            .add_mint_nft_transaction("alice", &nft_id);

        // Mine the NFT transaction
        server_state.blockchain.mine_pending_transactions("miner");

        // Verify the NFT
        server_state.nft_registry.verify_nft(
            &nft_id,
            "validator".to_string(),
            "valid-signature-123".to_string(),
            "This NFT's weather forecast looks accurate.".to_string(),
        );

        // Add verification transaction
        server_state
            .blockchain
            .add_verify_nft_transaction("validator", &nft_id);

        // Mine the verification
        server_state
            .blockchain
            .mine_pending_transactions("validator");

        // Create a weather forecast task and assign it to Sage
        let task = AgentTask::new(
            "Weekly Global Weather Analysis".to_string(),
            "Analyze global weather patterns for the coming week with focus on solar energy production regions".to_string(),
            nimbus_id,  // Use the stored Nimbus agent ID
            AgentTaskType::WeatherForecast,
            8, // Importance (1-10)
            None, // No deadline
        );

        let task_id = task.id.clone();
        server_state.agent_registry.add_task(task);
        let task = server_state.agent_registry.get_task_mut(&task_id).unwrap();
        task.assign(sage_id);
        task.start_progress();

        println!("Genesis block created. Blockchain initialized.");
        println!(
            "Alice balance: {}",
            server_state.blockchain.get_balance("alice")
        );
        println!(
            "Bob balance: {}",
            server_state.blockchain.get_balance("bob")
        );
        println!(
            "Miner balance: {}",
            server_state.blockchain.get_balance("miner")
        );
        println!(
            "Validator balance: {}",
            server_state.blockchain.get_balance("validator")
        );
        println!("Sample NFT created with ID: {nft_id}");
        println!("AI Agents initialized (Nova, Nimbus, Pulse, Sage)");
        println!("Sample task created from Nimbus to Sage");

        // Initialize sample nodes for network visualization
        server_state.node_registry.initialize_sample_nodes();
        println!(
            "Network nodes initialized ({} total)",
            server_state.node_registry.total_nodes
        );
    }

    // Start HTTP server
    let addr = SocketAddr::from(([0, 0, 0, 0], 8000));
    let listener = TcpListener::bind(addr).await?;
    println!("Wrfcoin server running on {addr}");
    println!("Available endpoints:");
    println!("  GET / - Blockchain status");
    println!("  GET /blocks - List blocks");
    println!("  GET /balance/:address - Get address balance");
    println!("  GET /nfts - List all NFTs");
    println!("  GET /nfts/:id - Get NFT details");
    println!("  GET /nfts/creator/:address - List NFTs by creator");
    println!("  GET /agents - List all AI agents");
    println!("  GET /agents/:id - Get agent details");
    println!("  GET /agents/role/:role - List agents by role");
    println!("  GET /tasks - List all tasks");
    println!("  GET /tasks/:id - Get task details");
    println!("  GET /tasks/agent/:agent_id - List tasks for agent");
    println!("  POST /transaction - Create transaction");
    println!("  POST /mine/:address - Mine pending transactions");
    println!("  POST /nfts/create - Create new NFT");
    println!("  POST /nfts/:id/verify - Verify an NFT");
    println!("  POST /nfts/:id/endorse - Endorse an NFT");
    println!("  POST /nfts/:id/view - Record a view of an NFT");
    println!("  POST /agents/create - Create new agent");
    println!("  POST /tasks/create - Create new task");
    println!("  POST /tasks/:id/assign - Assign task to agent");
    println!("  POST /tasks/:id/complete - Complete task");
    println!("  GET /resources - Get resource usage");
    println!("  GET /resources/tasks - List computational tasks");
    println!("  POST /resources/cpu - Set CPU limit");
    println!("  POST /resources/gpu - Set GPU limit");
    println!("  POST /resources/memory - Set memory limit");
    println!("  POST /resources/max-tasks - Set max concurrent tasks");
    println!("  GET /nodes - List all network nodes");
    println!("  GET /nodes/:id - Get node details");
    println!("  GET /nodes/type/:type - List nodes by type");
    println!("  GET /nodes/region/:region - List nodes by region");
    println!("  GET /nodes/metrics - Get network metrics");

    let state_clone = state.clone();

    loop {
        let (mut socket, _) = listener.accept().await?;
        let state = state_clone.clone();

        tokio::spawn(async move {
            handle_connection(&mut socket, state)
                .await
                .unwrap_or_else(|e| {
                    eprintln!("Error handling connection: {e}");
                });
        });
    }
}

/// Helper function to send a formatted HTTP response
async fn send_response(
    stream: &mut TcpStream,
    status_code: u16,
    content_type: &str,
    body: &str,
) -> io::Result<()> {
    let response = format_response(status_code, content_type, body);
    stream.write_all(response.as_bytes()).await?;
    stream.flush().await?;
    Ok(())
}

#[allow(clippy::excessive_nesting)]
async fn handle_connection(
    stream: &mut TcpStream,
    state: Arc<RwLock<ServerState>>,
) -> io::Result<()> {
    let mut buffer = [0; 4096]; // Increased buffer size for larger requests
    let n = stream.read(&mut buffer).await?;
    let data = String::from_utf8_lossy(&buffer[..n]);

    // Simple request parsing
    let lines: Vec<&str> = data.split("\r\n").collect();
    if lines.is_empty() {
        return Ok(());
    }

    let request_line: Vec<&str> = lines[0].split_whitespace().collect();
    if request_line.len() < 2 {
        return Ok(());
    }

    let method = request_line[0];
    let path = request_line[1];

    // Extract request body if present
    let mut body = String::new();
    let mut headers_done = false;
    for line in &lines[1..] {
        if line.is_empty() {
            headers_done = true;
            continue;
        }
        if headers_done {
            body.push_str(line);
            body.push_str("\r\n");
        }
    }

    // Parse body parameters if it's a form submission
    let mut params = HashMap::new();
    if !body.is_empty() && body.contains('=') {
        for param in body.split('&') {
            if let Some(equals_pos) = param.find('=') {
                let key = &param[0..equals_pos];
                let value = &param[equals_pos + 1..];
                params.insert(key.to_string(), value.to_string());
            }
        }
    }

    // Process the request and send the response
    let response = match (method, path) {
        // Add transactions and mine for testing the block UI
        ("GET", "/test/add-transactions") => {
            let mut server_state = state.write().await;
            // Add a few test transactions
            server_state
                .blockchain
                .add_transfer_transaction("alice", "bob", 1);
            server_state
                .blockchain
                .add_transfer_transaction("bob", "alice", 2);
            server_state
                .blockchain
                .add_transfer_transaction("validator", "miner", 3);

            format_response(
                200,
                "text/plain",
                "Added 3 test transactions to the mempool. Use /test/mine to mine them.",
            )
        },

        ("GET", "/test/add-nft") => {
            let mut server_state = state.write().await;
            // Create a test NFT block
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("Time went backwards")
                .as_secs();

            let nft_id = format!("nft-test-{now}");

            // Create a basic NFT with required fields matching the struct definition
            let mut metadata = HashMap::new();
            metadata.insert("type".to_string(), "test".to_string());
            metadata.insert("resolution".to_string(), "1080p".to_string());

            let test_nft = ComputableNFT {
                id: nft_id.clone(),
                title: "Test NFT".to_string(),
                creator: "alice".to_string(),
                timestamp: now,
                description: "This is a test NFT created for UI testing".to_string(),
                input_data_hashes: vec!["data1".to_string(), "data2".to_string()],
                executable_code: "function test() { return 'hello world'; }".to_string(),
                outputs: "test-outputs".to_string(),
                metadata,
                provenance: vec!["origin-test".to_string()],
                verification_status: VerificationStatus::Pending,
                engagement_metrics: EngagementMetrics::default(),
            };

            // Register the NFT
            server_state.nft_registry.add_nft(test_nft.clone());

            // Create a simplified JSON string representation of the NFT for the block data
            let data = format!(
                "{{\"id\":\"{}\",\"title\":\"{}\",\"creator\":\"{}\"}}",
                test_nft.id, test_nft.title, test_nft.creator
            );

            // Add a block directly to the blockchain using the existing method
            server_state
                .blockchain
                .add_block(data, BlockType::NFT(nft_id.clone()));

            format_response(
                200,
                "text/plain",
                &format!(
                    "Created a new NFT block with ID: {}. Return to dashboard to see it.",
                    nft_id
                ),
            )
        },

        ("GET", "/test/mine") => {
            let mut server_state = state.write().await;
            // Mine pending transactions
            server_state.blockchain.mine_pending_transactions("miner");

            format_response(
                200,
                "text/plain",
                "Mined a new block with the pending transactions. Return to dashboard to see it.",
            )
        },

        // Blockchain endpoints
        ("GET", "/") | ("GET", "/dashboard") => {
            let server_state = state.read().await;
            let blockchain = &server_state.blockchain;
            let nft_registry = &server_state.nft_registry;
            let agent_registry = &server_state.agent_registry;
            let resource_manager = &server_state.resource_manager;

            // Render HTML dashboard
            let dashboard_html = templates::render_dashboard(
                blockchain,
                nft_registry,
                agent_registry,
                resource_manager,
            );

            format_response(200, "text/html", &dashboard_html)
        },

        // Documents
        ("GET", "/documents") => {
            let server_state = state.read().await;
            let blockchain = &server_state.blockchain;

            // Render HTML documents page
            let documents_html = templates::render_documents(blockchain);

            format_response(200, "text/html", &documents_html)
        },

        // Network Health Dashboard
        ("GET", "/network-health") => {
            let server_state = state.read().await;
            let blockchain = &server_state.blockchain;
            let node_registry = &server_state.node_registry;

            // Render HTML network health dashboard
            let network_health_html = templates::render_network_health(blockchain, node_registry);

            format_response(200, "text/html", &network_health_html)
        },

        ("GET", "/status") => {
            let server_state = state.read().await;
            let blockchain = &server_state.blockchain;
            let nft_registry = &server_state.nft_registry;
            let agent_registry = &server_state.agent_registry;
            let resource_manager = &server_state.resource_manager;
            let node_registry = &server_state.node_registry;

            // Get resource usage
            let usage = resource_manager.get_current_usage();

            let status = format!(
                "Wrfcoin Blockchain Status: \n\
                 --------------------- \n\
                 Blockchain: \n\
                 - Blocks: {} \n\
                 - Is Valid: {} \n\
                 - Difficulty: {} \n\
                 - Pending Transactions: {} \n\n\
                 NFT Registry: \n\
                 - Total NFTs: {} \n\
                 - Verified NFTs: {} \n\
                 - Pending NFTs: {} \n\
                 - Rejected NFTs: {} \n\n\
                 Agent Framework: \n\
                 - Total Agents: {} \n\
                 - Total Tasks: {} \n\n\
                 Network Nodes: \n\
                 - Total Nodes: {} \n\
                 - Active Nodes: {} \n\
                 - Decentralization Score: {}/100 \n\n\
                 Resource Management: \n\
                 - CPU: {}% (limit: {}%) \n\
                 - GPU: {}% (limit: {}%) \n\
                 - Memory: {} MB (limit: {} MB) \n\
                 - Monitoring Active: {} \n\
                 - Check /resources for more details",
                blockchain.chain.len(),
                blockchain.is_valid(),
                blockchain.difficulty,
                blockchain.mempool.len(),
                nft_registry.nfts.len(),
                nft_registry.get_verified_nfts().len(),
                nft_registry.get_pending_nfts().len(),
                nft_registry.get_rejected_nfts().len(),
                agent_registry.agents.len(),
                agent_registry.tasks.len(),
                node_registry.total_nodes,
                node_registry.get_active_nodes().len(),
                node_registry.decentralization_score,
                usage.cpu_usage,
                resource_manager.get_cpu_limit(),
                usage.gpu_usage,
                resource_manager.get_gpu_limit(),
                usage.memory_usage,
                resource_manager.get_memory_limit(),
                resource_manager.is_monitoring_active()
            );

            format_response(200, "text/plain", &status)
        },

        ("GET", "/blocks") => {
            let server_state = state.read().await;
            let blockchain = &server_state.blockchain;
            let mut blocks_str = String::new();

            for block in &blockchain.chain {
                let block_type_str = match &block.block_type {
                    BlockType::Transaction => "Transaction Block".to_string(),
                    BlockType::NFT(id) => format!("NFT Block ({id})"),
                };

                blocks_str.push_str(&format!(
                    "Block #{}: \n\
                     - Type: {} \n\
                     - Hash: {} \n\
                     - Previous Hash: {} \n\
                     - Data: {} \n\
                     - Nonce: {} \n\n",
                    block.index,
                    block_type_str,
                    block.hash,
                    block.prev_hash,
                    if block.data.len() > 100 {
                        format!("{}...", &block.data[0..100])
                    } else {
                        block.data.clone()
                    },
                    block.nonce
                ));
            }

            format_response(200, "text/plain", &blocks_str)
        },

        ("GET", path) if path.starts_with("/block/") => {
            let parts: Vec<&str> = path.split('/').collect();
            if parts.len() >= 3 {
                if let Ok(block_idx) = parts[2].parse::<usize>() {
                    let server_state = state.read().await;
                    let blockchain = &server_state.blockchain;
                    let block_html = templates::render_block_details(blockchain, block_idx);
                    format_response(200, "text/html", &block_html)
                } else {
                    format_response(400, "text/plain", "Invalid block index")
                }
            } else {
                format_response(400, "text/plain", "Invalid block path")
            }
        },

        ("GET", path) if path.starts_with("/nft/") => {
            let parts: Vec<&str> = path.split('/').collect();
            if parts.len() >= 3 {
                let nft_id = parts[2];
                let server_state = state.read().await;
                let nft_registry = &server_state.nft_registry;
                let nft_html = templates::render_nft_details(nft_registry, nft_id);
                format_response(200, "text/html", &nft_html)
            } else {
                format_response(400, "text/plain", "Invalid NFT path")
            }
        },

        ("GET", path) if path.starts_with("/balance/") => {
            let address = &path[9..]; // Extract address from path
            let server_state = state.read().await;
            let balance = server_state.blockchain.get_balance(address);

            let response_str = format!("Balance of {address}: {balance}");
            format_response(200, "text/plain", &response_str)
        },

        // Agent endpoints
        ("GET", "/agents") => {
            let server_state = state.read().await;
            let agent_registry = &server_state.agent_registry;
            let agents = agent_registry.get_all_agents();

            let mut agents_str = format!("Total Agents: {}\n\n", agents.len());

            for agent in agents {
                agents_str.push_str(&format!(
                    "Agent: {} ({})\n\
                     - ID: {}\n\
                     - Role: {}\n\
                     - Reputation: {}\n\
                     - Owner: {}\n\
                     - Capabilities: {}\n\
                     - Contributed NFTs: {}\n\n",
                    agent.name,
                    agent.role.display_name(),
                    agent.id,
                    agent.role.description(),
                    agent.reputation,
                    agent.owner.as_deref().unwrap_or("None"),
                    agent.capabilities.join(", "),
                    agent.contributed_nfts.len()
                ));
            }

            format_response(200, "text/plain", &agents_str)
        },

        ("GET", path) if path.starts_with("/agents/") && path.contains("/role/") => {
            let parts: Vec<&str> = path.split('/').collect();
            if parts.len() >= 4 {
                let role_name = parts[3].to_lowercase();

                // Convert role string to AgentRole
                let role = match role_name.as_str() {
                    "nova" | "onboarding" => Some(AgentRole::Onboarding),
                    "pulse" | "marketing" => Some(AgentRole::Marketing),
                    "nimbus" | "weather" => Some(AgentRole::Weather),
                    "solace" | "publishing" => Some(AgentRole::Publishing),
                    "sage" | "research" => Some(AgentRole::Research),
                    "echo" | "governance" => Some(AgentRole::Governance),
                    "orbit" | "taskgeneration" => Some(AgentRole::TaskGeneration),
                    "quanta" | "tournament" => Some(AgentRole::Tournament),
                    _ => None,
                };

                if let Some(role) = role {
                    let server_state = state.read().await;
                    let agent_registry = &server_state.agent_registry;
                    let agents = agent_registry.get_agents_by_role(&role);

                    let mut agents_str = format!(
                        "Agents with role {}: {}\n\n",
                        role.display_name(),
                        agents.len()
                    );

                    for agent in agents {
                        agents_str.push_str(&format!(
                            "Agent: {} ({})\n\
                             - ID: {}\n\
                             - Reputation: {}\n\
                             - Owner: {}\n\
                             - Capabilities: {}\n\n",
                            agent.name,
                            agent.role.display_name(),
                            agent.id,
                            agent.reputation,
                            agent.owner.as_deref().unwrap_or("None"),
                            agent.capabilities.join(", ")
                        ));
                    }

                    format_response(200, "text/plain", &agents_str)
                } else {
                    format_response(400, "text/plain", &format!("Invalid role: {role_name}"))
                }
            } else {
                format_response(400, "text/plain", "Invalid role path")
            }
        },

        ("GET", path) if path.starts_with("/agents/") => {
            let parts: Vec<&str> = path.split('/').collect();
            if parts.len() >= 3 {
                let agent_id = parts[2];

                let server_state = state.read().await;
                let agent_registry = &server_state.agent_registry;

                if let Some(agent) = agent_registry.get_agent(agent_id) {
                    // Get tasks created by this agent
                    let created_tasks = agent_registry.get_tasks_by_creator(&agent.id);

                    // Get tasks assigned to this agent
                    let assigned_tasks = agent_registry.get_tasks_by_assignee(&agent.id);

                    let agent_details = format!(
                        "# Agent: {} ({})\n\n\
                         ## Basic Information\n\
                         - ID: {}\n\
                         - Created at: {}\n\
                         - Role: {}\n\
                         - Owner: {}\n\n\
                         ## Capabilities\n\
                         {}\n\n\
                         ## Performance\n\
                         - Reputation: {}\n\
                         - Contributed NFTs: {}\n\
                         - Last active: {}\n\n\
                         ## Tasks\n\
                         - Created: {} tasks\n\
                         - Assigned: {} tasks\n",
                        agent.name,
                        agent.role.display_name(),
                        agent.id,
                        agent.created_at,
                        agent.role.description(),
                        agent.owner.as_deref().unwrap_or("None"),
                        agent
                            .capabilities
                            .iter()
                            .map(|c| format!("- {c}"))
                            .collect::<Vec<String>>()
                            .join("\n"),
                        agent.reputation,
                        agent.contributed_nfts.len(),
                        agent.last_active,
                        created_tasks.len(),
                        assigned_tasks.len()
                    );

                    format_response(200, "text/plain", &agent_details)
                } else {
                    format_response(
                        404,
                        "text/plain",
                        &format!("Agent with ID {agent_id} not found"),
                    )
                }
            } else {
                format_response(400, "text/plain", "Invalid agent ID path")
            }
        },

        // Task endpoints
        ("GET", "/tasks") => {
            let server_state = state.read().await;
            let agent_registry = &server_state.agent_registry;
            let tasks = agent_registry.get_all_tasks();

            let mut tasks_str = format!("Total Tasks: {}\n\n", tasks.len());

            for task in tasks {
                let status_str = match &task.status {
                    AgentTaskStatus::Created => "Created",
                    AgentTaskStatus::Assigned => "Assigned",
                    AgentTaskStatus::InProgress => "In Progress",
                    AgentTaskStatus::Completed => "Completed",
                    AgentTaskStatus::Failed => "Failed",
                    AgentTaskStatus::Expired => "Expired",
                };

                let task_type_str = match &task.task_type {
                    AgentTaskType::WeatherForecast => "Weather Forecast",
                    AgentTaskType::MarketAnalysis => "Market Analysis",
                    AgentTaskType::ContentCreation => "Content Creation",
                    AgentTaskType::DataVisualization => "Data Visualization",
                    AgentTaskType::GovernanceProposal => "Governance Proposal",
                    AgentTaskType::NetworkMonitoring => "Network Monitoring",
                    AgentTaskType::ReputationAssessment => "Reputation Assessment",
                    AgentTaskType::TournamentCreation => "Tournament Creation",
                    AgentTaskType::Custom(name) => name,
                };

                tasks_str.push_str(&format!(
                    "Task: {}\n\
                     - ID: {}\n\
                     - Type: {}\n\
                     - Status: {}\n\
                     - Creator: {}\n\
                     - Assigned to: {}\n\
                     - Importance: {}/10\n\n",
                    task.title,
                    task.id,
                    task_type_str,
                    status_str,
                    task.agent_id,
                    task.assigned_to.as_deref().unwrap_or("None"),
                    task.importance
                ));
            }

            format_response(200, "text/plain", &tasks_str)
        },

        ("GET", path) if path.starts_with("/tasks/") && path.contains("/agent/") => {
            let parts: Vec<&str> = path.split('/').collect();
            if parts.len() >= 4 {
                let agent_id = parts[3];

                let server_state = state.read().await;
                let agent_registry = &server_state.agent_registry;

                // Check if the agent exists
                if agent_registry.get_agent(agent_id).is_none() {
                    stream
                        .write_all(
                            format_response(
                                404,
                                "text/plain",
                                &format!("Agent with ID {agent_id} not found"),
                            )
                            .as_bytes(),
                        )
                        .await?;
                    return Ok(());
                }

                // Get tasks created by this agent
                let created_tasks = agent_registry.get_tasks_by_creator(agent_id);

                // Get tasks assigned to this agent
                let assigned_tasks = agent_registry.get_tasks_by_assignee(agent_id);

                let mut tasks_str = format!(
                    "Tasks for agent {}:\n\
                     - Created: {} tasks\n\
                     - Assigned: {} tasks\n\n",
                    agent_id,
                    created_tasks.len(),
                    assigned_tasks.len()
                );

                if !created_tasks.is_empty() {
                    tasks_str.push_str("## Created Tasks\n\n");
                    for task in created_tasks {
                        tasks_str.push_str(&format!(
                            "Task: {}\n\
                             - ID: {}\n\
                             - Status: {}\n\
                             - Assigned to: {}\n\n",
                            task.title,
                            task.id,
                            format!("{:?}", task.status),
                            task.assigned_to.as_deref().unwrap_or("None")
                        ));
                    }
                }

                if !assigned_tasks.is_empty() {
                    tasks_str.push_str("## Assigned Tasks\n\n");
                    for task in assigned_tasks {
                        tasks_str.push_str(&format!(
                            "Task: {}\n\
                             - ID: {}\n\
                             - Status: {}\n\
                             - Creator: {}\n\n",
                            task.title,
                            task.id,
                            format!("{:?}", task.status),
                            task.agent_id
                        ));
                    }
                }

                format_response(200, "text/plain", &tasks_str)
            } else {
                format_response(400, "text/plain", "Invalid agent ID path")
            }
        },

        ("GET", path) if path.starts_with("/tasks/") => {
            let parts: Vec<&str> = path.split('/').collect();
            if parts.len() >= 3 {
                let task_id = parts[2];

                let server_state = state.read().await;
                let agent_registry = &server_state.agent_registry;

                if let Some(task) = agent_registry.get_task(task_id) {
                    let status_str = match &task.status {
                        AgentTaskStatus::Created => "Created",
                        AgentTaskStatus::Assigned => "Assigned",
                        AgentTaskStatus::InProgress => "In Progress",
                        AgentTaskStatus::Completed => "Completed",
                        AgentTaskStatus::Failed => "Failed",
                        AgentTaskStatus::Expired => "Expired",
                    };

                    let task_type_str = match &task.task_type {
                        AgentTaskType::WeatherForecast => "Weather Forecast",
                        AgentTaskType::MarketAnalysis => "Market Analysis",
                        AgentTaskType::ContentCreation => "Content Creation",
                        AgentTaskType::DataVisualization => "Data Visualization",
                        AgentTaskType::GovernanceProposal => "Governance Proposal",
                        AgentTaskType::NetworkMonitoring => "Network Monitoring",
                        AgentTaskType::ReputationAssessment => "Reputation Assessment",
                        AgentTaskType::TournamentCreation => "Tournament Creation",
                        AgentTaskType::Custom(name) => name,
                    };

                    let deadline_str = if let Some(deadline) = task.deadline {
                        format!("{deadline}")
                    } else {
                        "None".to_string()
                    };

                    let completed_at_str = if let Some(completed_at) = task.completed_at {
                        format!("{completed_at}")
                    } else {
                        "Not completed".to_string()
                    };

                    let result_nft_str = if let Some(nft_id) = &task.result_nft_id {
                        format!("{nft_id}")
                    } else {
                        "None".to_string()
                    };

                    let task_details = format!(
                        "# Task: {}\n\n\
                         ## Basic Information\n\
                         - ID: {}\n\
                         - Created at: {}\n\
                         - Type: {}\n\
                         - Status: {}\n\n\
                         ## Description\n\
                         {}\n\n\
                         ## Assignment\n\
                         - Created by: {}\n\
                         - Assigned to: {}\n\
                         - Importance: {}/10\n\
                         - Deadline: {}\n\n\
                         ## Results\n\
                         - Completed at: {}\n\
                         - Result NFT: {}\n",
                        task.title,
                        task.id,
                        task.created_at,
                        task_type_str,
                        status_str,
                        task.description,
                        task.agent_id,
                        task.assigned_to.as_deref().unwrap_or("None"),
                        task.importance,
                        deadline_str,
                        completed_at_str,
                        result_nft_str
                    );

                    format_response(200, "text/plain", &task_details)
                } else {
                    format_response(
                        404,
                        "text/plain",
                        &format!("Task with ID {task_id} not found"),
                    )
                }
            } else {
                format_response(400, "text/plain", "Invalid task ID path")
            }
        },

        // NFT endpoints
        ("GET", "/nfts") => {
            let server_state = state.read().await;
            let nft_registry = &server_state.nft_registry;
            let nfts = nft_registry.get_all_nfts();

            let mut nfts_str = format!("Total NFTs: {}\n\n", nfts.len());

            for nft in nfts {
                let verification_status = match &nft.verification_status {
                    VerificationStatus::Pending => "Pending".to_string(),
                    VerificationStatus::Verified(verifications) => {
                        format!("Verified ({} verifications)", verifications.len())
                    },
                    VerificationStatus::Rejected(reason) => format!("Rejected: {reason}"),
                };

                nfts_str.push_str(&format!(
                    "NFT ID: {} \n\
                     - Title: {} \n\
                     - Creator: {} \n\
                     - Status: {} \n\
                     - Total Reward: {} WRF \n\
                     - Views: {} \n\n",
                    nft.id,
                    nft.title,
                    nft.creator,
                    verification_status,
                    nft.calculate_total_reward(),
                    nft.engagement_metrics.views
                ));
            }

            format_response(200, "text/plain", &nfts_str)
        },

        ("GET", path) if path.starts_with("/nfts/") && path.contains("/creator/") => {
            let parts: Vec<&str> = path.split('/').collect();
            if parts.len() >= 4 {
                let creator_address = parts[3];

                let server_state = state.read().await;
                let nft_registry = &server_state.nft_registry;
                let nfts = nft_registry.get_nfts_by_creator(creator_address);

                let mut nfts_str =
                    format!("NFTs created by {}: {}\n\n", creator_address, nfts.len());

                for nft in nfts {
                    nfts_str.push_str(&format!(
                        "NFT ID: {} \n\
                         - Title: {} \n\
                         - Status: {} \n\
                         - Total Reward: {} WRF \n\n",
                        nft.id,
                        nft.title,
                        match &nft.verification_status {
                            VerificationStatus::Pending => "Pending",
                            VerificationStatus::Verified(_) => "Verified",
                            VerificationStatus::Rejected(_) => "Rejected",
                        },
                        nft.calculate_total_reward()
                    ));
                }

                format_response(200, "text/plain", &nfts_str)
            } else {
                format_response(400, "text/plain", "Invalid creator address path")
            }
        },

        ("GET", path) if path.starts_with("/nfts/") => {
            let parts: Vec<&str> = path.split('/').collect();
            if parts.len() >= 3 {
                let nft_id = parts[2];

                let server_state = state.read().await;
                let nft_registry = &server_state.nft_registry;

                if let Some(_nft) = nft_registry.get_nft(nft_id) {
                    // Record a view for the NFT (would be better to do this with a POST, but for demo)
                    drop(server_state); // Release read lock
                    let mut server_state = state.write().await;
                    server_state.nft_registry.record_view(nft_id);

                    // Get the updated NFT
                    let nft = server_state.nft_registry.get_nft(nft_id).unwrap();

                    let verification_details = match &nft.verification_status {
                        VerificationStatus::Pending => "Status: Pending verification".to_string(),
                        VerificationStatus::Verified(verifications) => {
                            let mut details = format!(
                                "Status: Verified ({} verifications)\n",
                                verifications.len()
                            );
                            for (i, v) in verifications.iter().enumerate() {
                                details.push_str(&format!(
                                    "Verification #{}: \n\
                                     - Verifier: {} \n\
                                     - Timestamp: {} \n\
                                     - Feedback: {} \n",
                                    i + 1,
                                    v.verifier,
                                    v.timestamp,
                                    v.feedback
                                ));
                            }
                            details
                        },
                        VerificationStatus::Rejected(reason) => {
                            format!("Status: Rejected\nReason: {reason}")
                        },
                    };

                    let mut endorsements = String::new();
                    if !nft.engagement_metrics.endorsements.is_empty() {
                        endorsements.push_str("Endorsements:\n");
                        for (endorser, message) in &nft.engagement_metrics.endorsements {
                            endorsements.push_str(&format!("- {endorser}: {message}\n"));
                        }
                    }

                    let nft_details = format!(
                        "# Computable NFT: {} \n\n\
                         ## Basic Information \n\
                         - ID: {} \n\
                         - Creator: {} \n\
                         - Created at: {} \n\n\
                         ## Content \n\
                         - Title: {} \n\
                         - Description: {} \n\
                         - Input Data Hashes: {} \n\
                         - Executable Code: \n```\n{}\n```\n\
                         - Outputs: \n```\n{}\n```\n\n\
                         ## Metadata \n\
                         {}\n\n\
                         ## Verification \n\
                         {}\n\n\
                         ## Engagement \n\
                         - Views: {} \n\
                         - Reuses: {} \n\
                         {}\n\n\
                         ## Rewards \n\
                         - Fixed Reward: {} WRF \n\
                         - Dynamic Reward: {} WRF \n\
                         - Total Reward: {} WRF \n",
                        nft.title,
                        nft.id,
                        nft.creator,
                        nft.timestamp,
                        nft.title,
                        nft.description,
                        nft.input_data_hashes.join(", "),
                        nft.executable_code,
                        nft.outputs,
                        nft.metadata
                            .iter()
                            .map(|(k, v)| format!("- {k}: {v}"))
                            .collect::<Vec<String>>()
                            .join("\n"),
                        verification_details,
                        nft.engagement_metrics.views,
                        nft.engagement_metrics.reuses,
                        endorsements,
                        nft.calculate_fixed_reward(),
                        nft.calculate_dynamic_reward(),
                        nft.calculate_total_reward()
                    );

                    format_response(200, "text/plain", &nft_details)
                } else {
                    format_response(
                        404,
                        "text/plain",
                        &format!("NFT with ID {nft_id} not found"),
                    )
                }
            } else {
                format_response(400, "text/plain", "Invalid NFT ID path")
            }
        },

        // Transaction endpoints
        ("POST", "/transaction") => {
            if params.contains_key("from")
                && params.contains_key("to")
                && params.contains_key("amount")
            {
                let from = params.get("from").unwrap();
                let to = params.get("to").unwrap();
                let amount = params.get("amount").unwrap().parse::<u64>().unwrap_or(0);

                let mut server_state = state.write().await;
                let success = server_state
                    .blockchain
                    .add_transfer_transaction(from, to, amount);

                let status = if success {
                    format!(
                        "Transaction added: {} sent {} coins to {}",
                        from, amount, to
                    )
                } else {
                    format!("Transaction failed: Insufficient balance")
                };

                format_response(200, "text/plain", &status)
            } else {
                format_response(
                    400,
                    "text/plain",
                    "Missing required parameters: from, to, amount",
                )
            }
        },

        ("POST", path) if path.starts_with("/mine/") => {
            let address = &path[6..]; // Extract mining reward address

            let mut server_state = state.write().await;
            match server_state.blockchain.mine_pending_transactions(address) {
                Some(block) => {
                    let status = format!(
                        "Block mined successfully: \n\
                         - Block #{} \n\
                         - Hash: {} \n\
                         - Mining reward sent to: {} \n\
                         - New balance: {}",
                        block.index,
                        block.hash,
                        address,
                        server_state.blockchain.get_balance(address)
                    );

                    format_response(200, "text/plain", &status)
                },
                None => {
                    let status = "No transactions to mine";
                    format_response(200, "text/plain", &status)
                },
            }
        },

        // NFT manipulation endpoints
        ("POST", "/nfts/create") => {
            if params.contains_key("creator")
                && params.contains_key("title")
                && params.contains_key("description")
                && params.contains_key("code")
                && params.contains_key("outputs")
            {
                let creator = params.get("creator").unwrap();
                let title = params.get("title").unwrap();
                let description = params.get("description").unwrap();
                let code = params.get("code").unwrap();
                let outputs = params.get("outputs").unwrap();

                // Parse additional parameters
                let input_data_hashes = if params.contains_key("input_hashes") {
                    params
                        .get("input_hashes")
                        .unwrap()
                        .split(',')
                        .map(|s| s.to_string())
                        .collect()
                } else {
                    vec![]
                };

                let provenance = if params.contains_key("provenance") {
                    params
                        .get("provenance")
                        .unwrap()
                        .split(',')
                        .map(|s| s.to_string())
                        .collect()
                } else {
                    vec![]
                };

                // Create metadata from any param starting with "meta_"
                let mut metadata = HashMap::new();
                for (key, value) in &params {
                    if key.starts_with("meta_") {
                        metadata.insert(key[5..].to_string(), value.clone());
                    }
                }

                // Create new NFT
                let nft = ComputableNFT::new(
                    creator.clone(),
                    title.clone(),
                    description.clone(),
                    input_data_hashes,
                    code.clone(),
                    outputs.clone(),
                    metadata,
                    provenance,
                );

                let nft_id = nft.id.clone();

                // Add NFT to registry
                let mut server_state = state.write().await;
                server_state.nft_registry.add_nft(nft);

                // Create blockchain transaction for minting
                server_state
                    .blockchain
                    .add_mint_nft_transaction(creator, &nft_id);

                let status = format!(
                    "NFT created successfully: \n\
                     - ID: {} \n\
                     - Title: {} \n\
                     - Creator: {} \n\
                     - Status: Pending verification",
                    nft_id, title, creator
                );

                format_response(200, "text/plain", &status)
            } else {
                format_response(
                    400,
                    "text/plain",
                    "Missing required parameters: creator, title, description, code, outputs",
                )
            }
        },

        ("POST", path) if path.starts_with("/nfts/") && path.contains("/verify") => {
            let parts: Vec<&str> = path.split('/').collect();
            if parts.len() >= 4 && parts[3] == "verify" {
                let nft_id = parts[2];

                if params.contains_key("verifier") && params.contains_key("feedback") {
                    let verifier = params.get("verifier").unwrap();
                    let feedback = params.get("feedback").unwrap();

                    // Create simple signature
                    let signature = format!("sig-verify-{verifier}-{nft_id}");

                    let mut server_state = state.write().await;
                    let success = server_state.nft_registry.verify_nft(
                        nft_id,
                        verifier.clone(),
                        signature.clone(),
                        feedback.clone(),
                    );

                    if success {
                        // Add verification transaction
                        server_state
                            .blockchain
                            .add_verify_nft_transaction(verifier, nft_id);

                        let status = format!(
                            "NFT verified successfully: \n\
                             - ID: {} \n\
                             - Verifier: {} \n\
                             - Feedback: {}",
                            nft_id, verifier, feedback
                        );

                        format_response(200, "text/plain", &status)
                    } else {
                        format_response(
                            404,
                            "text/plain",
                            &format!("NFT with ID {nft_id} not found"),
                        )
                    }
                } else {
                    format_response(
                        400,
                        "text/plain",
                        "Missing required parameters: verifier, feedback",
                    )
                }
            } else {
                format_response(400, "text/plain", "Invalid NFT verify path")
            }
        },

        ("POST", path) if path.starts_with("/nfts/") && path.contains("/endorse") => {
            let parts: Vec<&str> = path.split('/').collect();
            if parts.len() >= 4 && parts[3] == "endorse" {
                let nft_id = parts[2];

                if params.contains_key("endorser") && params.contains_key("message") {
                    let endorser = params.get("endorser").unwrap();
                    let message = params.get("message").unwrap();

                    let mut server_state = state.write().await;
                    let success = server_state.nft_registry.add_endorsement(
                        nft_id,
                        endorser.clone(),
                        message.clone(),
                    );

                    if success {
                        // Add endorsement transaction
                        server_state
                            .blockchain
                            .add_endorse_nft_transaction(endorser, nft_id);

                        let status = format!(
                            "NFT endorsed successfully: \n\
                             - ID: {} \n\
                             - Endorser: {} \n\
                             - Message: {}",
                            nft_id, endorser, message
                        );

                        format_response(200, "text/plain", &status)
                    } else {
                        format_response(
                            404,
                            "text/plain",
                            &format!("NFT with ID {nft_id} not found"),
                        )
                    }
                } else {
                    format_response(
                        400,
                        "text/plain",
                        "Missing required parameters: endorser, message",
                    )
                }
            } else {
                format_response(400, "text/plain", "Invalid NFT endorse path")
            }
        },

        ("POST", path) if path.starts_with("/nfts/") && path.contains("/view") => {
            let parts: Vec<&str> = path.split('/').collect();
            if parts.len() >= 4 && parts[3] == "view" {
                let nft_id = parts[2];

                let mut server_state = state.write().await;
                let success = server_state.nft_registry.record_view(nft_id);

                if success {
                    let nft = server_state.nft_registry.get_nft(nft_id).unwrap();
                    let status = format!(
                        "NFT view recorded: \n\
                         - ID: {} \n\
                         - Total Views: {}",
                        nft_id, nft.engagement_metrics.views
                    );

                    format_response(200, "text/plain", &status)
                } else {
                    format_response(
                        404,
                        "text/plain",
                        &format!("NFT with ID {nft_id} not found"),
                    )
                }
            } else {
                format_response(400, "text/plain", "Invalid NFT view path")
            }
        },

        // Agent manipulation endpoints
        ("POST", "/agents/create") => {
            if params.contains_key("name")
                && params.contains_key("role")
                && params.contains_key("capabilities")
            {
                let name = params.get("name").unwrap();
                let role_str = params.get("role").unwrap().to_lowercase();
                let capabilities = params
                    .get("capabilities")
                    .unwrap()
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .collect::<Vec<String>>();
                let owner = params.get("owner").map(|o| o.to_string());

                // Convert role string to AgentRole
                let role = match role_str.as_str() {
                    "nova" | "onboarding" => AgentRole::Onboarding,
                    "pulse" | "marketing" => AgentRole::Marketing,
                    "nimbus" | "weather" => AgentRole::Weather,
                    "solace" | "publishing" => AgentRole::Publishing,
                    "sage" | "research" => AgentRole::Research,
                    "echo" | "governance" => AgentRole::Governance,
                    "orbit" | "taskgeneration" => AgentRole::TaskGeneration,
                    "quanta" | "tournament" => AgentRole::Tournament,
                    _ => AgentRole::Research, // Default to Research role
                };

                // Create new agent
                let agent = Agent::new(name.clone(), role, capabilities.clone(), owner.clone());

                let agent_id = agent.id.clone();

                // Add agent to registry
                let mut server_state = state.write().await;
                server_state.agent_registry.add_agent(agent);

                let status = format!(
                    "Agent created successfully: \n\
                     - ID: {} \n\
                     - Name: {} \n\
                     - Role: {} \n\
                     - Owner: {} \n\
                     - Capabilities: {}",
                    agent_id,
                    name,
                    match role_str.as_str() {
                        "nova" | "onboarding" => "Onboarding Agent",
                        "pulse" | "marketing" => "Marketing Agent",
                        "nimbus" | "weather" => "Weather Analysis Agent",
                        "solace" | "publishing" => "Publishing Agent",
                        "sage" | "research" => "Research Agent",
                        "echo" | "governance" => "Governance Agent",
                        "orbit" | "taskgeneration" => "Task Generation Agent",
                        "quanta" | "tournament" => "Tournament Agent",
                        _ => "Research Agent", // Default
                    },
                    owner.unwrap_or_else(|| "None".to_string()),
                    capabilities.join(", ")
                );

                format_response(200, "text/plain", &status)
            } else {
                format_response(
                    400,
                    "text/plain",
                    "Missing required parameters: name, role, capabilities",
                )
            }
        },

        // Task manipulation endpoints
        ("POST", "/tasks/create") => {
            if params.contains_key("title")
                && params.contains_key("description")
                && params.contains_key("creator_agent_id")
                && params.contains_key("task_type")
            {
                let title = params.get("title").unwrap();
                let description = params.get("description").unwrap();
                let creator_agent_id = params.get("creator_agent_id").unwrap();
                let task_type_str = params.get("task_type").unwrap().to_lowercase();
                let importance = params
                    .get("importance")
                    .map(|i| i.parse::<u8>().unwrap_or(5))
                    .unwrap_or(5);
                let deadline = params.get("deadline").and_then(|d| d.parse::<u64>().ok());

                // Create task type
                let task_type = match task_type_str.as_str() {
                    "weather" | "weatherforecast" => AgentTaskType::WeatherForecast,
                    "market" | "marketanalysis" => AgentTaskType::MarketAnalysis,
                    "content" | "contentcreation" => AgentTaskType::ContentCreation,
                    "visualization" | "datavisualization" => AgentTaskType::DataVisualization,
                    "governance" | "governanceproposal" => AgentTaskType::GovernanceProposal,
                    "network" | "networkmonitoring" => AgentTaskType::NetworkMonitoring,
                    "reputation" | "reputationassessment" => AgentTaskType::ReputationAssessment,
                    "tournament" | "tournamentcreation" => AgentTaskType::TournamentCreation,
                    _ => AgentTaskType::Custom(task_type_str.clone()),
                };

                // Create new task
                let task = AgentTask::new(
                    title.clone(),
                    description.clone(),
                    creator_agent_id.clone(),
                    task_type,
                    importance,
                    deadline,
                );

                let task_id = task.id.clone();

                // Check if the creator agent exists
                let mut server_state = state.write().await;
                if server_state
                    .agent_registry
                    .get_agent(creator_agent_id)
                    .is_none()
                {
                    return send_response(
                        stream,
                        404,
                        "text/plain",
                        &format!("Agent with ID {creator_agent_id} not found"),
                    )
                    .await;
                }

                // Add task to registry
                server_state.agent_registry.add_task(task);

                let status = format!(
                    "Task created successfully: \n\
                     - ID: {} \n\
                     - Title: {} \n\
                     - Creator: {} \n\
                     - Status: Created \n\
                     - Importance: {}/10",
                    task_id, title, creator_agent_id, importance
                );

                format_response(200, "text/plain", &status)
            } else {
                format_response(
                    400,
                    "text/plain",
                    "Missing required parameters: title, description, creator_agent_id, task_type",
                )
            }
        },

        ("POST", path) if path.starts_with("/tasks/") && path.contains("/assign") => {
            let parts: Vec<&str> = path.split('/').collect();
            if parts.len() >= 4 && parts[3] == "assign" {
                let task_id = parts[2];

                if params.contains_key("agent_id") {
                    let agent_id = params.get("agent_id").unwrap();

                    let mut server_state = state.write().await;

                    // Check if task exists
                    if server_state.agent_registry.get_task(task_id).is_none() {
                        return send_response(
                            stream,
                            404,
                            "text/plain",
                            &format!("Task with ID {task_id} not found"),
                        )
                        .await;
                    }

                    // Check if agent exists
                    if server_state.agent_registry.get_agent(agent_id).is_none() {
                        return send_response(
                            stream,
                            404,
                            "text/plain",
                            &format!("Agent with ID {agent_id} not found"),
                        )
                        .await;
                    }

                    // Assign the task
                    let task = server_state.agent_registry.get_task_mut(task_id).unwrap();
                    task.assign(agent_id.clone());

                    let status = format!(
                        "Task assigned successfully: \n\
                         - Task ID: {} \n\
                         - Assigned to: {} \n\
                         - Status: Assigned",
                        task_id, agent_id
                    );

                    format_response(200, "text/plain", &status)
                } else {
                    format_response(400, "text/plain", "Missing required parameter: agent_id")
                }
            } else {
                format_response(400, "text/plain", "Invalid task assign path")
            }
        },

        ("POST", path) if path.starts_with("/tasks/") && path.contains("/complete") => {
            let parts: Vec<&str> = path.split('/').collect();
            if parts.len() >= 4 && parts[3] == "complete" {
                let task_id = parts[2];

                if params.contains_key("result_nft_id") {
                    let result_nft_id = params.get("result_nft_id").unwrap();

                    let mut server_state = state.write().await;

                    // Check if task exists
                    if server_state.agent_registry.get_task(task_id).is_none() {
                        return send_response(
                            stream,
                            404,
                            "text/plain",
                            &format!("Task with ID {task_id} not found"),
                        )
                        .await;
                    }

                    // Check if NFT exists
                    if server_state.nft_registry.get_nft(result_nft_id).is_none() {
                        return send_response(
                            stream,
                            404,
                            "text/plain",
                            &format!("NFT with ID {result_nft_id} not found"),
                        )
                        .await;
                    }

                    // Get info we need before completing the task
                    let importance;
                    let assigned_agent_id;

                    {
                        let task = server_state.agent_registry.get_task(task_id).unwrap();
                        importance = task.importance;
                        assigned_agent_id = task.assigned_to.clone();
                    }

                    // Complete the task
                    {
                        let task = server_state.agent_registry.get_task_mut(task_id).unwrap();
                        task.complete(Some(result_nft_id.clone()));
                    }

                    // Update the agent's reputation if applicable
                    if let Some(agent_id) = assigned_agent_id {
                        if let Some(agent) = server_state.agent_registry.get_agent_mut(&agent_id) {
                            agent.increase_reputation(importance as u64);
                            agent.add_contributed_nft(result_nft_id.clone());
                        }
                    }

                    let status = format!(
                        "Task completed successfully: \n\
                         - Task ID: {} \n\
                         - Result NFT: {} \n\
                         - Status: Completed",
                        task_id, result_nft_id
                    );

                    format_response(200, "text/plain", &status)
                } else {
                    format_response(
                        400,
                        "text/plain",
                        "Missing required parameter: result_nft_id",
                    )
                }
            } else {
                format_response(400, "text/plain", "Invalid task complete path")
            }
        },

        // Resource management endpoints
        ("GET", "/resources") => {
            let server_state = state.read().await;
            let resource_manager = &server_state.resource_manager;
            let computation_scheduler = &server_state.computation_scheduler;

            // Get current resource usage
            let usage = resource_manager.get_current_usage();

            // Get resource limits
            let cpu_limit = resource_manager.get_cpu_limit();
            let gpu_limit = resource_manager.get_gpu_limit();
            let memory_limit = resource_manager.get_memory_limit();

            // Get computation scheduler status
            let scheduler_status = computation_scheduler.read().await.get_status_summary();

            let resource_status = format!(
                "## Resource Status\n\n\
                 ### Current Usage\n\
                 - CPU: {}% (limit: {}%)\n\
                 - GPU: {}% (limit: {}%)\n\
                 - Memory: {} MB (limit: {} MB)\n\
                 - Timestamp: {}\n\n\
                 ### Computation Scheduler\n\
                 {}\n",
                usage.cpu_usage,
                cpu_limit,
                usage.gpu_usage,
                gpu_limit,
                usage.memory_usage,
                memory_limit,
                usage.timestamp,
                scheduler_status
            );

            format_response(200, "text/plain", &resource_status)
        },

        ("GET", "/resources/tasks") => {
            let server_state = state.read().await;
            let computation_scheduler = &server_state.computation_scheduler;

            // Get task registry - storing the lock in a variable to extend its lifetime
            let scheduler_lock = computation_scheduler.read().await;
            let task_registry = scheduler_lock.get_task_registry();
            let tasks = task_registry.get_all_tasks();

            let mut tasks_str = format!(
                "## Available Computational Tasks\n\nTotal Tasks: {}\n\n",
                tasks.len()
            );

            for task in tasks {
                // Get priority string
                let priority_str = match task.priority {
                    ComputationPriority::Low => "Low",
                    ComputationPriority::Medium => "Medium",
                    ComputationPriority::High => "High",
                    ComputationPriority::Critical => "Critical",
                };

                tasks_str.push_str(&format!(
                    "### {}\n\
                     - Description: {}\n\
                     - Priority: {}\n\
                     - Resource Requirements:\n\
                     - - CPU: {}%\n\
                     - - GPU: {}%\n\
                     - - Memory: {} MB\n\
                     - - Requires GPU: {}\n\n",
                    task.name,
                    task.description,
                    priority_str,
                    task.estimated_cpu_usage,
                    task.estimated_gpu_usage,
                    task.estimated_memory_usage,
                    if task.requires_gpu { "Yes" } else { "No" }
                ));
            }

            format_response(200, "text/plain", &tasks_str)
        },

        ("POST", "/resources/cpu") => {
            if params.contains_key("limit") {
                if let Ok(limit) = params.get("limit").unwrap().parse::<u8>() {
                    let server_state = state.read().await;
                    let resource_manager = &server_state.resource_manager;

                    // Set the CPU limit
                    resource_manager.set_cpu_limit(limit);

                    let status = format!(
                        "CPU limit set successfully: {}%\n\
                         The node will now use up to {}% of CPU for computational tasks.",
                        limit, limit
                    );

                    format_response(200, "text/plain", &status)
                } else {
                    format_response(
                        400,
                        "text/plain",
                        "Invalid CPU limit. Please provide a number between 0 and 100.",
                    )
                }
            } else {
                format_response(400, "text/plain", "Missing required parameter: limit")
            }
        },

        ("POST", "/resources/gpu") => {
            if params.contains_key("limit") {
                if let Ok(limit) = params.get("limit").unwrap().parse::<u8>() {
                    let server_state = state.read().await;
                    let resource_manager = &server_state.resource_manager;

                    // Set the GPU limit
                    match resource_manager.set_gpu_limit(limit) {
                        Ok(_) => {
                            let status = format!(
                                "GPU limit set successfully: {}%\n\
                                 The node will now use up to {}% of GPU for computational tasks.",
                                limit, limit
                            );

                            format_response(200, "text/plain", &status)
                        },
                        Err(e) => format_response(
                            400,
                            "text/plain",
                            &format!("Error setting GPU limit: {e}"),
                        ),
                    }
                } else {
                    format_response(
                        400,
                        "text/plain",
                        "Invalid GPU limit. Please provide a number between 0 and 100.",
                    )
                }
            } else {
                format_response(400, "text/plain", "Missing required parameter: limit")
            }
        },

        ("POST", "/resources/memory") => {
            if params.contains_key("limit") {
                if let Ok(limit) = params.get("limit").unwrap().parse::<u16>() {
                    let server_state = state.read().await;
                    let resource_manager = &server_state.resource_manager;

                    // Set the memory limit
                    resource_manager.set_memory_limit(limit);

                    let status = format!(
                        "Memory limit set successfully: {} MB\n\
                         The node will now use up to {} MB of memory for computational tasks.",
                        limit, limit
                    );

                    format_response(200, "text/plain", &status)
                } else {
                    format_response(
                        400,
                        "text/plain",
                        "Invalid memory limit. Please provide a number in MB.",
                    )
                }
            } else {
                format_response(400, "text/plain", "Missing required parameter: limit")
            }
        },

        ("POST", "/resources/max-tasks") => {
            if params.contains_key("limit") {
                if let Ok(limit) = params.get("limit").unwrap().parse::<usize>() {
                    let server_state = state.read().await;
                    let computation_scheduler = &server_state.computation_scheduler;

                    // Set the max concurrent tasks
                    computation_scheduler
                        .write()
                        .await
                        .set_max_concurrent_tasks(limit);

                    let status = format!(
                        "Maximum concurrent tasks set successfully: {}\n\
                         The node will now run up to {} computational tasks simultaneously.",
                        limit, limit
                    );

                    format_response(200, "text/plain", &status)
                } else {
                    format_response(
                        400,
                        "text/plain",
                        "Invalid task limit. Please provide a positive number.",
                    )
                }
            } else {
                format_response(400, "text/plain", "Missing required parameter: limit")
            }
        },

        // Node endpoints
        ("GET", "/nodes") => {
            let server_state = state.read().await;
            let node_registry = &server_state.node_registry;
            let nodes = node_registry.get_all_nodes();

            let mut nodes_str = format!("Total Nodes: {}\n\n", nodes.len());

            for node in nodes {
                nodes_str.push_str(&format!(
                    "Node: {} ({})\n\
                     - ID: {}\n\
                     - Type: {:?}\n\
                     - Status: {:?}\n\
                     - Region: {:?}\n\
                     - Version: {}\n\
                     - Reputation: {}\n\
                     - Uptime: {} days\n\n",
                    node.name,
                    node.ip_prefix,
                    node.id,
                    node.node_type,
                    node.status,
                    node.region,
                    node.version,
                    node.metrics.reputation,
                    node.metrics.uptime / 86400
                ));
            }

            format_response(200, "text/plain", &nodes_str)
        },

        ("GET", path) if path.starts_with("/nodes/") && path.contains("/type/") => {
            let parts: Vec<&str> = path.split('/').collect();
            if parts.len() >= 4 {
                use nodes::NodeType;

                let type_name = parts[3].to_lowercase();

                // Convert type string to NodeType
                let node_type = match type_name.as_str() {
                    "validator" => Some(NodeType::Validator),
                    "analyzer" => Some(NodeType::Analyzer),
                    "dataprovider" => Some(NodeType::DataProvider),
                    "developer" => Some(NodeType::Developer),
                    "light" => Some(NodeType::Light),
                    _ => None,
                };

                if let Some(node_type) = node_type {
                    let server_state = state.read().await;
                    let node_registry = &server_state.node_registry;
                    let nodes = node_registry.get_nodes_by_type(&node_type);

                    let mut nodes_str =
                        format!("Nodes of type {:?}: {}\n\n", node_type, nodes.len());

                    for node in nodes {
                        nodes_str.push_str(&format!(
                            "Node: {} ({})\n\
                             - ID: {}\n\
                             - Status: {:?}\n\
                             - Region: {:?}\n\
                             - Reputation: {}\n\
                             - Blocks processed: {}\n\
                             - Transactions: {}\n\n",
                            node.name,
                            node.ip_prefix,
                            node.id,
                            node.status,
                            node.region,
                            node.metrics.reputation,
                            node.metrics.blocks_processed,
                            node.metrics.transactions_processed
                        ));
                    }

                    format_response(200, "text/plain", &nodes_str)
                } else {
                    format_response(
                        400,
                        "text/plain",
                        &format!("Invalid node type: {type_name}"),
                    )
                }
            } else {
                format_response(400, "text/plain", "Invalid node type path")
            }
        },

        ("GET", path) if path.starts_with("/nodes/") && path.contains("/region/") => {
            let parts: Vec<&str> = path.split('/').collect();
            if parts.len() >= 4 {
                use nodes::Region;

                let region_name = parts[3].to_lowercase();

                // Convert region string to Region
                let region = match region_name.as_str() {
                    "northamerica" => Some(Region::NorthAmerica),
                    "southamerica" => Some(Region::SouthAmerica),
                    "europe" => Some(Region::Europe),
                    "asia" => Some(Region::Asia),
                    "africa" => Some(Region::Africa),
                    "oceania" => Some(Region::Oceania),
                    _ => None,
                };

                if let Some(region) = region {
                    let server_state = state.read().await;
                    let node_registry = &server_state.node_registry;
                    let nodes = node_registry.get_nodes_by_region(&region);

                    let mut nodes_str =
                        format!("Nodes in region {:?}: {}\n\n", region, nodes.len());

                    for node in nodes {
                        nodes_str.push_str(&format!(
                            "Node: {} ({})\n\
                             - ID: {}\n\
                             - Type: {:?}\n\
                             - Status: {:?}\n\
                             - Reputation: {}\n\
                             - Joined: {}\n\n",
                            node.name,
                            node.ip_prefix,
                            node.id,
                            node.node_type,
                            node.status,
                            node.metrics.reputation,
                            node.metrics
                                .joined_at
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs()
                                / 86400
                        ));
                    }

                    format_response(200, "text/plain", &nodes_str)
                } else {
                    format_response(
                        400,
                        "text/plain",
                        &format!("Invalid region: {region_name}"),
                    )
                }
            } else {
                format_response(400, "text/plain", "Invalid region path")
            }
        },

        ("GET", path) if path.starts_with("/nodes/") && path.ends_with("/metrics") => {
            let server_state = state.read().await;
            let node_registry = &server_state.node_registry;

            let active_nodes = node_registry.get_active_nodes();
            let validator_nodes = node_registry.get_nodes_by_type(&nodes::NodeType::Validator);
            let analyzer_nodes = node_registry.get_nodes_by_type(&nodes::NodeType::Analyzer);
            let data_provider_nodes =
                node_registry.get_nodes_by_type(&nodes::NodeType::DataProvider);
            let developer_nodes = node_registry.get_nodes_by_type(&nodes::NodeType::Developer);
            let light_nodes = node_registry.get_nodes_by_type(&nodes::NodeType::Light);

            let regional_distribution = node_registry.get_regional_distribution();

            // Format regional distribution
            let mut regions_str = String::new();
            for (region, count) in regional_distribution {
                regions_str.push_str(&format!(
                    "  - {:?}: {} nodes ({}%)\n",
                    region,
                    count,
                    ((*count as f64 / node_registry.total_nodes as f64) * 100.0).round()
                ));
            }

            // Network health metrics
            let metrics = format!(
                "# Wrfcoin Network Metrics\n\n\
                 ## Node Distribution\n\
                 - Total nodes: {}\n\
                 - Active nodes: {} ({}%)\n\
                 - Validator nodes: {} ({}%)\n\
                 - Analyzer nodes: {} ({}%)\n\
                 - Data Provider nodes: {} ({}%)\n\
                 - Developer nodes: {} ({}%)\n\
                 - Light nodes: {} ({}%)\n\n\
                 ## Regional Distribution\n\
                 {}\n\
                 ## Network Health\n\
                 - Decentralization score: {}/100\n\
                 - Average node uptime: {} days\n\
                 - Average node reputation: {}\n\
                 - Total blocks processed: {}\n\
                 - Total transactions processed: {}\n\
                 - Total data contributions: {}\n\
                 - Total computations: {}\n",
                node_registry.total_nodes,
                active_nodes.len(),
                ((active_nodes.len() as f64 / node_registry.total_nodes as f64) * 100.0).round(),
                validator_nodes.len(),
                ((validator_nodes.len() as f64 / node_registry.total_nodes as f64) * 100.0).round(),
                analyzer_nodes.len(),
                ((analyzer_nodes.len() as f64 / node_registry.total_nodes as f64) * 100.0).round(),
                data_provider_nodes.len(),
                ((data_provider_nodes.len() as f64 / node_registry.total_nodes as f64) * 100.0)
                    .round(),
                developer_nodes.len(),
                ((developer_nodes.len() as f64 / node_registry.total_nodes as f64) * 100.0).round(),
                light_nodes.len(),
                ((light_nodes.len() as f64 / node_registry.total_nodes as f64) * 100.0).round(),
                regions_str,
                node_registry.decentralization_score,
                node_registry.get_average_uptime() / 86400,
                node_registry.get_average_reputation(),
                node_registry.get_total_blocks_processed(),
                node_registry.get_total_transactions_processed(),
                node_registry.get_total_data_contributions(),
                node_registry.get_total_computations()
            );

            format_response(200, "text/plain", &metrics)
        },

        ("GET", "/nodes/metrics") => {
            let server_state = state.read().await;
            let node_registry = &server_state.node_registry;

            let active_nodes = node_registry.get_active_nodes();
            let validator_nodes = node_registry.get_nodes_by_type(&nodes::NodeType::Validator);
            let analyzer_nodes = node_registry.get_nodes_by_type(&nodes::NodeType::Analyzer);
            let data_provider_nodes =
                node_registry.get_nodes_by_type(&nodes::NodeType::DataProvider);
            let developer_nodes = node_registry.get_nodes_by_type(&nodes::NodeType::Developer);
            let light_nodes = node_registry.get_nodes_by_type(&nodes::NodeType::Light);

            let regional_distribution = node_registry.get_regional_distribution();

            // Format regional distribution
            let mut regions_str = String::new();
            for (region, count) in regional_distribution {
                regions_str.push_str(&format!(
                    "  - {:?}: {} nodes ({}%)\n",
                    region,
                    count,
                    ((*count as f64 / node_registry.total_nodes as f64) * 100.0).round()
                ));
            }

            // Network health metrics
            let metrics = format!(
                "# Wrfcoin Network Metrics\n\n\
                 ## Node Distribution\n\
                 - Total nodes: {}\n\
                 - Active nodes: {} ({}%)\n\
                 - Validator nodes: {} ({}%)\n\
                 - Analyzer nodes: {} ({}%)\n\
                 - Data Provider nodes: {} ({}%)\n\
                 - Developer nodes: {} ({}%)\n\
                 - Light nodes: {} ({}%)\n\n\
                 ## Regional Distribution\n\
                 {}\n\
                 ## Network Health\n\
                 - Decentralization score: {}/100\n\
                 - Average node uptime: {} days\n\
                 - Average node reputation: {:.2}\n\
                 - Total blocks processed: {}\n\
                 - Total transactions processed: {}\n\
                 - Total data contributions: {}\n\
                 - Total computations: {}\n",
                node_registry.total_nodes,
                active_nodes.len(),
                ((active_nodes.len() as f64 / node_registry.total_nodes as f64) * 100.0).round(),
                validator_nodes.len(),
                ((validator_nodes.len() as f64 / node_registry.total_nodes as f64) * 100.0).round(),
                analyzer_nodes.len(),
                ((analyzer_nodes.len() as f64 / node_registry.total_nodes as f64) * 100.0).round(),
                data_provider_nodes.len(),
                ((data_provider_nodes.len() as f64 / node_registry.total_nodes as f64) * 100.0)
                    .round(),
                developer_nodes.len(),
                ((developer_nodes.len() as f64 / node_registry.total_nodes as f64) * 100.0).round(),
                light_nodes.len(),
                ((light_nodes.len() as f64 / node_registry.total_nodes as f64) * 100.0).round(),
                regions_str,
                node_registry.decentralization_score,
                node_registry.get_average_uptime() / 86400,
                node_registry.get_average_reputation(),
                node_registry.get_total_blocks_processed(),
                node_registry.get_total_transactions_processed(),
                node_registry.get_total_data_contributions(),
                node_registry.get_total_computations()
            );

            format_response(200, "text/plain", &metrics)
        },

        ("GET", path)
            if path.starts_with("/nodes/")
                && !path.contains("/type/")
                && !path.contains("/region/")
                && !path.ends_with("/metrics") =>
        {
            let parts: Vec<&str> = path.split('/').collect();
            if parts.len() >= 3 {
                let node_id = parts[2];

                let server_state = state.read().await;
                let node_registry = &server_state.node_registry;

                if let Some(node) = node_registry.get_node(node_id) {
                    let node_details = format!(
                        "# Node: {} ({:?})\n\n\
                         ## Basic Information\n\
                         - ID: {}\n\
                         - Type: {:?}\n\
                         - Status: {:?}\n\
                         - Region: {:?}\n\
                         - IP Prefix: {}\n\
                         - Version: {}\n\n\
                         ## Performance Metrics\n\
                         - Uptime: {} days\n\
                         - Reputation: {}\n\
                         - Blocks processed: {}\n\
                         - Transactions processed: {}\n\
                         - Data contributions: {}\n\
                         - Computations: {}\n\
                         - Upgrades proposed: {}\n\
                         - Upgrades accepted: {}\n\
                         - Joined at: {}\n\
                         - Last seen: {}\n",
                        node.name,
                        node.node_type,
                        node.id,
                        node.node_type,
                        node.status,
                        node.region,
                        node.ip_prefix,
                        node.version,
                        node.metrics.uptime / 86400,
                        node.metrics.reputation,
                        node.metrics.blocks_processed,
                        node.metrics.transactions_processed,
                        node.metrics.data_contributions,
                        node.metrics.computations,
                        node.metrics.upgrades_proposed,
                        node.metrics.upgrades_accepted,
                        node.metrics
                            .joined_at
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs(),
                        node.metrics
                            .last_seen
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs()
                    );

                    format_response(200, "text/plain", &node_details)
                } else {
                    format_response(
                        404,
                        "text/plain",
                        &format!("Node with ID {node_id} not found"),
                    )
                }
            } else {
                format_response(400, "text/plain", "Invalid node ID path")
            }
        },

        // Default 404 response
        _ => {
            let not_found = "404 Not Found: The requested resource was not found on this server.";
            format_response(404, "text/plain", not_found)
        },
    };

    // Parse the response into its components
    let response_parts: Vec<&str> = response.split("\r\n\r\n").collect();
    if response_parts.len() != 2 {
        return send_response(
            stream,
            500,
            "text/plain",
            "Internal server error: Invalid response format",
        )
        .await;
    }

    let headers_part = response_parts[0];
    let body_part = response_parts[1];

    // Extract the status code and content type
    let status_line_end = headers_part.find("\r\n").unwrap_or(headers_part.len());
    let status_line = &headers_part[0..status_line_end];
    let status_parts: Vec<&str> = status_line.split_whitespace().collect();

    if status_parts.len() < 3 {
        return send_response(
            stream,
            500,
            "text/plain",
            "Internal server error: Invalid status line",
        )
        .await;
    }

    let status_code = status_parts[1].parse::<u16>().unwrap_or(500);

    // Extract content type
    let content_type_pos = headers_part.find("Content-Type: ");
    let content_type = if let Some(pos) = content_type_pos {
        let start = pos + "Content-Type: ".len();
        let end = headers_part[start..]
            .find("\r\n")
            .unwrap_or(headers_part[start..].len());
        &headers_part[start..start + end]
    } else {
        "text/plain"
    };

    // Send the response
    send_response(stream, status_code, content_type, body_part).await
}

/// Helper function to format HTTP responses
fn format_response(status_code: u16, content_type: &str, body: &str) -> String {
    let status_text = match status_code {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "Unknown",
    };

    format!(
        "HTTP/1.1 {} {}\r\n\
         Content-Type: {}\r\n\
         Content-Length: {}\r\n\
         \r\n\
         {}",
        status_code,
        status_text,
        content_type,
        body.len(),
        body
    )
}

#[cfg(test)]
mod integration_harness_tests {
    use super::*;
    use crate::nft::VerificationStatus;
    use crate::nodes::{NodeStatus, NodeType};

    // ── Blockchain module tests ───────────────────────────────────────────────

    #[test]
    fn blockchain_transfer_and_mining_flow_is_executable() {
        let mut blockchain = Blockchain::new();
        blockchain.balances.insert("alice".to_string(), 100);
        blockchain.balances.insert("bob".to_string(), 10);

        assert!(blockchain.add_transfer_transaction("alice", "bob", 25));
        let mined = blockchain
            .mine_pending_transactions("miner")
            .expect("transfer tx should produce a block");

        assert!(matches!(mined.block_type, BlockType::Transaction));
        assert_eq!(blockchain.get_balance("alice"), 75);
        assert_eq!(blockchain.get_balance("bob"), 35);
        assert_eq!(blockchain.get_balance("miner"), 50);
        assert!(blockchain.is_valid());
    }

    #[test]
    fn multi_block_mining_preserves_chain_validity() {
        let mut blockchain = Blockchain::new();
        blockchain.balances.insert("alice".to_string(), 1000);

        // Mine several blocks in sequence
        for i in 0..5 {
            assert!(
                blockchain.add_transfer_transaction("alice", "bob", 10),
                "transfer {i} should succeed"
            );
            let block = blockchain
                .mine_pending_transactions("miner")
                .expect("should mine block");
            assert!(matches!(block.block_type, BlockType::Transaction));
        }

        assert!(blockchain.is_valid(), "chain should be valid after 5 blocks");
        assert_eq!(blockchain.get_balance("alice"), 950);
        assert_eq!(blockchain.get_balance("bob"), 50);
        assert_eq!(blockchain.get_balance("miner"), 250); // 50 reward * 5
    }

    #[test]
    fn transfer_with_insufficient_balance_fails() {
        let mut blockchain = Blockchain::new();
        blockchain.balances.insert("alice".to_string(), 10);

        let ok = blockchain.add_transfer_transaction("alice", "bob", 100);
        assert!(!ok, "transfer exceeding balance should fail");
        assert_eq!(blockchain.get_balance("alice"), 10);
    }

    #[test]
    fn mining_with_no_pending_transactions_returns_none() {
        let mut blockchain = Blockchain::new();
        let result = blockchain.mine_pending_transactions("miner");
        assert!(result.is_none(), "no pending txs should return None");
    }

    // ── NFT module tests ──────────────────────────────────────────────────────

    #[test]
    fn nft_registry_verification_and_rewards_flow_is_executable() {
        let mut registry = NFTRegistry::new();
        let mut metadata = HashMap::new();
        metadata.insert("task_type".to_string(), "weather_forecast".to_string());
        let nft = ComputableNFT::new(
            "alice".to_string(),
            "Forecast".to_string(),
            "Seven-day weather forecast".to_string(),
            vec!["ipfs://example".to_string()],
            "print('ok')".to_string(),
            "{\"result\":\"ok\"}".to_string(),
            metadata,
            vec!["oracle:demo".to_string()],
        );
        let nft_id = nft.id.clone();
        registry.add_nft(nft);

        assert!(registry.verify_nft(
            &nft_id,
            "validator".to_string(),
            "sig-1".to_string(),
            "looks good".to_string()
        ));
        assert!(registry.record_view(&nft_id));
        assert!(registry.record_reuse(&nft_id));
        assert!(registry.add_endorsement(
            &nft_id,
            "reviewer".to_string(),
            "useful forecast".to_string()
        ));

        let stored = registry.get_nft(&nft_id).expect("NFT should exist");
        assert!(matches!(
            stored.verification_status,
            VerificationStatus::Verified(_)
        ));
        assert!(stored.calculate_total_reward() > 0);
    }

    #[test]
    fn nft_mint_and_blockchain_record() {
        let mut blockchain = Blockchain::new();
        blockchain.balances.insert("creator".to_string(), 100);
        assert!(blockchain.add_mint_nft_transaction("creator", "nft-001"));

        let block = blockchain
            .mine_pending_transactions("miner")
            .expect("should mine NFT block");

        assert!(matches!(block.block_type, BlockType::NFT(_)));

        let nft_blocks = blockchain.get_nft_blocks("nft-001");
        assert_eq!(nft_blocks.len(), 1, "should have one NFT block");
    }

    #[test]
    fn nft_verify_and_endorse_chain_tracking() {
        let mut blockchain = Blockchain::new();
        blockchain.balances.insert("creator".to_string(), 100);
        blockchain.add_mint_nft_transaction("creator", "nft-002");
        blockchain.mine_pending_transactions("miner");

        assert!(blockchain.add_verify_nft_transaction("validator", "nft-002"));
        blockchain.mine_pending_transactions("miner");

        assert!(blockchain.add_endorse_nft_transaction("reviewer", "nft-002"));
        blockchain.mine_pending_transactions("miner");

        let nft_blocks = blockchain.get_nft_blocks("nft-002");
        assert_eq!(nft_blocks.len(), 3, "mint + verify + endorse = 3 NFT blocks");
        assert!(blockchain.is_valid());
    }

    // ── Agent module tests ────────────────────────────────────────────────────

    #[test]
    fn agent_task_lifecycle() {
        let mut registry = AgentRegistry::new();
        let agent = Agent::new(
            "Weatherbot".to_string(),
            AgentRole::Weather,
            vec!["sensor_read".to_string(), "data_validate".to_string()],
            None,
        );
        let agent_id = agent.id.clone();
        registry.add_agent(agent);

        let task = AgentTask::new(
            "Collect station data".to_string(),
            "Read all sensor values".to_string(),
            agent_id.clone(),
            AgentTaskType::NetworkMonitoring,
            1,
            Some(3600),
        );
        let task_id = task.id.clone();
        registry.add_task(task);

        // Assign and progress
        {
            let task = registry.get_task_mut(&task_id).unwrap();
            task.assign(agent_id.clone());
            assert!(matches!(task.status, AgentTaskStatus::Assigned));
            task.start_progress();
            assert!(matches!(task.status, AgentTaskStatus::InProgress));
            task.complete(Some("nft-result-001".to_string()));
            assert!(matches!(task.status, AgentTaskStatus::Completed));
        }

        let completed = registry.get_tasks_by_status(&AgentTaskStatus::Completed);
        assert_eq!(completed.len(), 1);
    }

    #[test]
    fn agent_reputation_tracks_contributions() {
        let mut registry = AgentRegistry::new();
        let mut agent = Agent::new(
            "Forecaster".to_string(),
            AgentRole::Research,
            vec!["predict".to_string()],
            None,
        );
        let agent_id = agent.id.clone();
        agent.increase_reputation(50);
        assert_eq!(agent.reputation, 50);
        agent.decrease_reputation(20);
        assert_eq!(agent.reputation, 30);
        agent.add_contributed_nft("nft-123".to_string());
        assert_eq!(agent.contributed_nfts.len(), 1);

        registry.add_agent(agent);
        let stored = registry.get_agent(&agent_id).unwrap();
        assert_eq!(stored.reputation, 30);
    }

    // ── Node registry tests ──────────────────────────────────────────────────

    #[test]
    fn dashboard_and_node_registry_paths_are_executable() {
        let mut blockchain = Blockchain::new();
        blockchain.balances.insert("alice".to_string(), 100);
        blockchain.balances.insert("bob".to_string(), 50);
        blockchain.add_transfer_transaction("alice", "bob", 5);
        let _ = blockchain.mine_pending_transactions("miner");

        let mut nft_registry = NFTRegistry::new();
        let nft = ComputableNFT::new(
            "alice".to_string(),
            "Model".to_string(),
            "Demo model".to_string(),
            vec!["ipfs://demo".to_string()],
            "print('demo')".to_string(),
            "{\"status\":\"ok\"}".to_string(),
            HashMap::new(),
            vec!["agent:nova".to_string()],
        );
        nft_registry.add_nft(nft);

        let mut agent_registry = AgentRegistry::new();
        agent_registry.add_agent(Agent::new(
            "Nova".to_string(),
            AgentRole::Onboarding,
            vec!["guide".to_string()],
            None,
        ));

        let mut node_registry = NodeRegistry::new();
        node_registry.initialize_sample_nodes();
        assert_eq!(node_registry.get_nodes_by_type(&NodeType::Validator).len(), 5);
        assert!(!node_registry.get_nodes_by_status(&NodeStatus::Active).is_empty());

        let resource_manager = ResourceManager::new();
        let html = templates::render_dashboard(
            &blockchain,
            &nft_registry,
            &agent_registry,
            &resource_manager,
        );
        assert!(html.contains("Wrfcoin Blockchain Dashboard"));
        assert!(html.contains("Recent Blocks"));
        assert!(html.contains("Latest NFTs"));
    }

    // ── Cross-module integration tests ────────────────────────────────────────

    #[test]
    fn agent_creates_nft_that_records_on_blockchain() {
        let mut blockchain = Blockchain::new();
        blockchain.balances.insert("creator".to_string(), 100);

        let mut nft_registry = NFTRegistry::new();
        let mut agent_registry = AgentRegistry::new();

        // Create an agent
        agent_registry.add_agent(Agent::new(
            "WeatherAI".to_string(),
            AgentRole::Weather,
            vec!["predict".to_string(), "validate".to_string()],
            None,
        ));

        // Agent creates an NFT
        let nft = ComputableNFT::new(
            "creator".to_string(),
            "7-Day Forecast".to_string(),
            "AI-generated weather forecast".to_string(),
            vec!["ipfs://forecast-data".to_string()],
            "model.predict(7)".to_string(),
            "{\"accuracy\":0.95}".to_string(),
            HashMap::new(),
            vec!["agent:WeatherAI".to_string()],
        );
        let nft_id = nft.id.clone();
        nft_registry.add_nft(nft);

        // Record NFT on blockchain
        assert!(blockchain.add_mint_nft_transaction("creator", &nft_id));
        blockchain.mine_pending_transactions("miner");

        // Verify the NFT
        assert!(nft_registry.verify_nft(
            &nft_id,
            "validator".to_string(),
            "sig-v1".to_string(),
            "forecast verified".to_string()
        ));
        assert!(blockchain.add_verify_nft_transaction("validator", &nft_id));
        blockchain.mine_pending_transactions("miner");

        // Validate cross-module consistency
        let nft_blocks = blockchain.get_nft_blocks(&nft_id);
        assert_eq!(nft_blocks.len(), 2, "mint + verify blocks");
        assert!(blockchain.is_valid());

        let stored_nft = nft_registry.get_nft(&nft_id).unwrap();
        assert!(matches!(
            stored_nft.verification_status,
            VerificationStatus::Verified(_)
        ));
    }

    #[test]
    fn resource_manager_enforces_limits() {
        let resource_manager = ResourceManager::new();
        resource_manager.set_cpu_limit(80);
        assert_eq!(resource_manager.get_cpu_limit(), 80);
        // Critical tasks always run regardless of limits
        assert!(resource_manager.can_run_computation(ComputationPriority::Critical));
        // Simulated usage = limit/2, so High priority (usage < limit) should pass
        assert!(resource_manager.can_run_computation(ComputationPriority::High));
    }

    #[test]
    fn full_platform_smoke_test() {
        // Instantiate every major module and verify they interoperate
        let mut blockchain = Blockchain::new();
        blockchain.balances.insert("alice".to_string(), 1000);
        blockchain.balances.insert("bob".to_string(), 500);

        let mut nft_registry = NFTRegistry::new();
        let mut agent_registry = AgentRegistry::new();
        let mut node_registry = NodeRegistry::new();
        let resource_manager = ResourceManager::new();

        // Set up nodes
        node_registry.initialize_sample_nodes();

        // Create agent
        agent_registry.add_agent(Agent::new(
            "SensorBot".to_string(),
            AgentRole::Weather,
            vec!["collect".to_string()],
            None,
        ));

        // Transfer funds
        assert!(blockchain.add_transfer_transaction("alice", "bob", 100));
        blockchain.mine_pending_transactions("miner");

        // Mint NFT
        let nft = ComputableNFT::new(
            "alice".to_string(),
            "SensorData".to_string(),
            "Weather sensor readings".to_string(),
            vec!["ipfs://sensors".to_string()],
            "process_readings()".to_string(),
            "{\"readings\":100}".to_string(),
            HashMap::new(),
            vec!["agent:SensorBot".to_string()],
        );
        let nft_id = nft.id.clone();
        nft_registry.add_nft(nft);
        blockchain.add_mint_nft_transaction("alice", &nft_id);
        blockchain.mine_pending_transactions("miner");

        // Render dashboard as integration check
        let html = templates::render_dashboard(
            &blockchain,
            &nft_registry,
            &agent_registry,
            &resource_manager,
        );

        // Verify all modules contributed to the output
        assert!(blockchain.is_valid());
        assert!(html.contains("Wrfcoin Blockchain Dashboard"));
        assert_eq!(blockchain.get_balance("alice"), 900);
        assert_eq!(blockchain.get_balance("bob"), 600);
        assert!(blockchain.get_balance("miner") > 0);
        assert!(nft_registry.get_nft(&nft_id).is_some());
        assert!(!node_registry.get_nodes_by_status(&NodeStatus::Active).is_empty());
    }
}
