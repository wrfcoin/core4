use crate::blockchain::{Blockchain, BlockType};
use crate::nft::{NFTRegistry, ComputableNFT, VerificationStatus};
use crate::agents::{AgentRegistry, Agent};
use crate::resources::ResourceManager;
use crate::nodes;

/// Renders the HTML for the dashboard page
pub fn render_dashboard(
    blockchain: &Blockchain,
    nft_registry: &NFTRegistry,
    agent_registry: &AgentRegistry,
    resource_manager: &ResourceManager,
) -> String {
    let html = format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Wrfcoin Dashboard</title>
    <style>
        body {{
            font-family: 'Segoe UI', Tahoma, Geneva, Verdana, sans-serif;
            line-height: 1.6;
            color: #333;
            max-width: 1200px;
            margin: 0 auto;
            padding: 20px;
            background-color: #f9f9f9;
        }}
        h1, h2, h3 {{
            color: #2c3e50;
        }}
        .dashboard {{
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(300px, 1fr));
            gap: 20px;
            margin-top: 20px;
        }}
        .card {{
            background: white;
            border-radius: 8px;
            box-shadow: 0 2px 4px rgba(0,0,0,0.1);
            padding: 20px;
            transition: transform 0.2s;
        }}
        .card:hover {{
            transform: translateY(-5px);
            box-shadow: 0 4px 8px rgba(0,0,0,0.15);
        }}
        .stat {{
            font-size: 1.8em;
            font-weight: bold;
            color: #3498db;
            margin: 10px 0;
        }}
        .label {{
            font-size: 0.9em;
            color: #7f8c8d;
            text-transform: uppercase;
        }}
        table {{
            width: 100%;
            border-collapse: collapse;
            margin-top: 20px;
        }}
        th, td {{
            text-align: left;
            padding: 12px 15px;
            border-bottom: 1px solid #ddd;
        }}
        th {{
            background-color: #f2f2f2;
        }}
        tr:hover {{
            background-color: #f5f5f5;
        }}
        a {{
            color: #3498db;
            text-decoration: none;
        }}
        a:hover {{
            text-decoration: underline;
        }}
        .navbar {{
            background-color: #2c3e50;
            padding: 15px 20px;
            border-radius: 8px;
            margin-bottom: 20px;
        }}
        .navbar a {{
            color: white;
            margin-right: 15px;
            padding: 8px 15px;
            border-radius: 4px;
        }}
        .navbar a:hover {{
            background-color: #3498db;
        }}
    </style>
</head>
<body>
    <div class="navbar">
        <a href="/">Dashboard</a>
        <a href="/blocks">Blocks</a>
        <a href="/nfts">NFTs</a>
        <a href="/agents">Agents</a>
        <a href="/resources">Resources</a>
        <a href="/documents">Documents</a>
        <a href="/network-health">Network Health</a>
    </div>

    <h1>Wrfcoin Blockchain Dashboard</h1>
    
    <div class="dashboard">
        <div class="card">
            <div class="label">Total Blocks</div>
            <div class="stat">{}</div>
            <a href="/blocks">View all blocks →</a>
        </div>
        
        <div class="card">
            <div class="label">Total NFTs</div>
            <div class="stat">{}</div>
            <a href="/nfts">View all NFTs →</a>
        </div>
        
        <div class="card">
            <div class="label">Active Agents</div>
            <div class="stat">{}</div>
            <a href="/agents">View all agents →</a>
        </div>
        
        <div class="card">
            <div class="label">CPU Usage</div>
            <div class="stat">{}%</div>
            <a href="/resources">View resources →</a>
        </div>
        
        <div class="card">
            <div class="label">Validator Nodes</div>
            <div class="stat">{}</div>
            <a href="/nodes?type=validator">View validators →</a>
        </div>
        
        <div class="card">
            <div class="label">Verification Success Rate</div>
            <div class="stat">{}%</div>
            <a href="/nfts/verified">View verified NFTs →</a>
        </div>
        
        <div class="card">
            <div class="label">Avg. Verification Time</div>
            <div class="stat">{} sec</div>
            <a href="/verification/metrics">View metrics →</a>
        </div>
        
        <div class="card">
            <div class="label">Accuracy Score (Avg)</div>
            <div class="stat">{}</div>
            <a href="/assessments">View assessments →</a>
        </div>
    </div>
    
    {}
    
    <h2>Recent Blocks</h2>
    <table>
        <tr>
            <th>Height</th>
            <th>Type</th>
            <th>Hash (truncated)</th>
            <th>Time</th>
            <th>Actions</th>
        </tr>
        {}
    </table>
    
    <h2>Latest NFTs</h2>
    <table>
        <tr>
            <th>ID</th>
            <th>Title</th>
            <th>Creator</th>
            <th>Status</th>
            <th>Actions</th>
        </tr>
        {}
    </table>
    
    <h2>Agent Activity</h2>
    <table>
        <tr>
            <th>Agent</th>
            <th>Role</th>
            <th>Tasks</th>
            <th>Reputation</th>
            <th>Actions</th>
        </tr>
        {}
    </table>
</body>
</html>"#,
        blockchain.chain.len(),
        nft_registry.nfts.len(),
        agent_registry.agents.len(),
        resource_manager.get_current_usage().cpu_usage,
        // Verification metrics - in a real implementation, these would come from the verification manager
        12, // Number of validator nodes
        95, // Verification success rate (percentage)
        8,  // Average verification time (seconds)
        87, // Average accuracy score
        render_block_status_panel(blockchain),
        render_recent_blocks(blockchain),
        render_latest_nfts(nft_registry),
        render_agent_activity(agent_registry)
    );
    
    html
}

/// Renders the HTML table rows for recent blocks
fn render_recent_blocks(blockchain: &Blockchain) -> String {
    let mut html = String::new();
    
    // Get up to 5 most recent blocks (in reverse order)
    let block_count = blockchain.chain.len();
    let start_idx = if block_count > 5 { block_count - 5 } else { 0 };
    
    for i in start_idx..block_count {
        let block = &blockchain.chain[i];
        let block_type = match block.block_type {
            BlockType::Transaction => "Transaction",
            BlockType::NFT(_) => "NFT",
        };
        
        // Truncate hash to first 10 chars for display
        let hash_preview = if block.hash.len() > 10 {
            &block.hash[0..10]
        } else {
            &block.hash
        };
        
        html.push_str(&format!(
            r#"<tr>
                <td>{}</td>
                <td>{}</td>
                <td title="{}">{}..</td>
                <td>{}</td>
                <td><a href="/block/{}">View Details</a></td>
            </tr>"#,
            i,
            block_type,
            block.hash,
            hash_preview,
            block.timestamp,
            i
        ));
    }
    
    html
}

/// Renders an HTML component showing the three most recently finalized blocks plus a live block
pub fn render_block_status_panel(blockchain: &Blockchain) -> String {
    let mut html = String::new();
    
    html.push_str(r#"
    <div class="block-status-panel">
        <h2>Blockchain Status</h2>
        <div class="test-actions">
            <a href="/test/add-transactions" class="btn btn-primary">Add Test Transactions</a>
            <a href="/test/mine" class="btn btn-secondary">Mine Block</a>
            <a href="/test/add-nft" class="btn btn-nft">Create NFT Block</a>
        </div>
        <div class="blocks-container">
    "#);
    
    // Get finalized blocks
    let block_count = blockchain.chain.len();
    let mut finalized_blocks = Vec::new();
    
    // Get up to 3 most recent finalized blocks
    if block_count > 0 {
        let start_idx = if block_count > 3 { block_count - 3 } else { 0 };
        for i in start_idx..block_count {
            finalized_blocks.push(&blockchain.chain[i]);
        }
    }
    
    // Generate CSS for block display
    html.push_str(r#"
    <style>
        .block-status-panel {
            margin: 25px 0;
            background: white;
            border-radius: 8px;
            box-shadow: 0 2px 10px rgba(0,0,0,0.1);
            padding: 20px;
        }
        .test-actions {
            display: flex;
            gap: 10px;
            margin-bottom: 15px;
        }
        .btn {
            display: inline-block;
            padding: 8px 16px;
            border-radius: 4px;
            font-size: 0.9em;
            text-decoration: none;
            cursor: pointer;
            transition: background-color 0.2s, transform 0.1s;
        }
        .btn:hover {
            transform: translateY(-2px);
        }
        .btn-primary {
            background-color: #3498db;
            color: white;
        }
        .btn-primary:hover {
            background-color: #2980b9;
        }
        .btn-secondary {
            background-color: #2ecc71;
            color: white;
        }
        .btn-secondary:hover {
            background-color: #27ae60;
        }
        .btn-nft {
            background-color: #9b59b6;
            color: white;
        }
        .btn-nft:hover {
            background-color: #8e44ad;
        }
        .blocks-container {
            display: flex;
            flex-wrap: wrap;
            gap: 15px;
            margin-top: 15px;
        }
        .block-card {
            flex: 1;
            min-width: 250px;
            border: 1px solid #e0e0e0;
            border-radius: 6px;
            padding: 15px;
            position: relative;
        }
        .block-card.live {
            border-color: #3498db;
            background-color: rgba(52, 152, 219, 0.05);
        }
        .live-indicator {
            position: absolute;
            top: 10px;
            right: 10px;
            background-color: #e74c3c;
            color: white;
            padding: 3px 8px;
            border-radius: 10px;
            font-size: 0.7em;
            font-weight: bold;
            animation: pulse 1.5s infinite;
        }
        @keyframes pulse {
            0% { opacity: 1; }
            50% { opacity: 0.5; }
            100% { opacity: 1; }
        }
        .block-header {
            font-weight: bold;
            margin-bottom: 10px;
            color: #2c3e50;
        }
        .block-property {
            display: flex;
            margin-bottom: 5px;
            font-size: 0.9em;
        }
        .block-property-name {
            width: 80px;
            color: #7f8c8d;
        }
        .block-property-value {
            flex: 1;
            word-break: break-all;
        }
        .block-hash {
            font-family: monospace;
            font-size: 0.85em;
            color: #555;
        }
        .pending-block {
            border-style: dashed;
            background-color: #f9f9f9;
        }
        .block-time {
            font-size: 0.8em;
            color: #7f8c8d;
            margin-top: 10px;
            text-align: right;
        }
    </style>
    "#);
    
    // Create a live block (currently being mined) if there are pending transactions
    if !blockchain.mempool.is_empty() {
        html.push_str(r#"
        <div class="block-card live">
            <div class="live-indicator">LIVE</div>
            <div class="block-header">Block #"#);
        html.push_str(&format!("{}", block_count));
        html.push_str(r#" (Mining)</div>
            
            <div class="block-property">
                <div class="block-property-name">Type:</div>
                <div class="block-property-value">Transaction</div>
            </div>
            
            <div class="block-property">
                <div class="block-property-name">Txns:</div>
                <div class="block-property-value">"#);
        html.push_str(&format!("{}", blockchain.mempool.len()));
        html.push_str(r#"</div>
            </div>
            
            <div class="block-property">
                <div class="block-property-name">Prev Hash:</div>
                <div class="block-property-value block-hash">"#);
        
        if !blockchain.chain.is_empty() {
            let prev_hash = &blockchain.chain[block_count-1].hash;
            html.push_str(&format!("{}...", &prev_hash[0..min(10, prev_hash.len())]));
        } else {
            html.push_str("Genesis");
        }
        
        html.push_str(r#"</div>
            </div>
            
            <div class="block-time">Mining in progress...</div>
        </div>
        "#);
    } else if block_count > 0 {
        // If no pending transactions, show the latest block as "live" with different styling
        let latest_block = &blockchain.chain[block_count-1];
        // Remove this block from finalized blocks since we'll show it as the live block
        finalized_blocks.pop();
        
        html.push_str(r#"
        <div class="block-card live">
            <div class="live-indicator">LATEST</div>
            <div class="block-header">Block #"#);
        html.push_str(&format!("{}", latest_block.index));
        html.push_str(r#"</div>
            
            <div class="block-property">
                <div class="block-property-name">Type:</div>
                <div class="block-property-value">"#);
        
        match &latest_block.block_type {
            BlockType::Transaction => html.push_str("Transaction"),
            BlockType::NFT(id) => {
                html.push_str("NFT");
                html.push_str(&format!(" ({})", id));
            }
        }
        
        html.push_str(r#"</div>
            </div>
            
            <div class="block-property">
                <div class="block-property-name">Hash:</div>
                <div class="block-property-value block-hash">"#);
        
        html.push_str(&format!("{}...", &latest_block.hash[0..min(10, latest_block.hash.len())]));
        
        html.push_str(r#"</div>
            </div>
            
            <div class="block-property">
                <div class="block-property-name">Prev Hash:</div>
                <div class="block-property-value block-hash">"#);
        
        if latest_block.index > 0 {
            html.push_str(&format!("{}...", &latest_block.prev_hash[0..min(10, latest_block.prev_hash.len())]));
        } else {
            html.push_str("Genesis");
        }
        
        html.push_str(r#"</div>
            </div>
            
            <div class="block-time">Mined at timestamp: "#);
        html.push_str(&format!("{}", latest_block.timestamp));
        html.push_str(r#"</div>
        </div>
        "#);
    }
    
    // Render finalized blocks
    for block in finalized_blocks.iter().rev() {  // Reverse to show oldest first
        html.push_str(r#"
        <div class="block-card">
            <div class="block-header">Block #"#);
        html.push_str(&format!("{}", block.index));
        html.push_str(r#"</div>
            
            <div class="block-property">
                <div class="block-property-name">Type:</div>
                <div class="block-property-value">"#);
        
        match &block.block_type {
            BlockType::Transaction => html.push_str("Transaction"),
            BlockType::NFT(id) => {
                html.push_str("NFT");
                html.push_str(&format!(" ({})", id));
            }
        }
        
        html.push_str(r#"</div>
            </div>
            
            <div class="block-property">
                <div class="block-property-name">Hash:</div>
                <div class="block-property-value block-hash">"#);
        
        html.push_str(&format!("{}...", &block.hash[0..min(10, block.hash.len())]));
        
        html.push_str(r#"</div>
            </div>
            
            <div class="block-time">Finalized at timestamp: "#);
        html.push_str(&format!("{}", block.timestamp));
        html.push_str(r#"</div>
        </div>
        "#);
    }
    
    // If no blocks at all, show a placeholder
    if blockchain.chain.is_empty() && blockchain.mempool.is_empty() {
        html.push_str(r#"
        <div class="block-card pending-block">
            <div class="block-header">No Blocks Yet</div>
            <div class="block-property">
                <div class="block-property-value">The blockchain hasn't been initialized with any blocks yet.</div>
            </div>
        </div>
        "#);
    }
    
    html.push_str(r#"
        </div>
    </div>
    "#);
    
    html
}

use std::cmp::min;

/// Renders the HTML table rows for latest NFTs
fn render_latest_nfts(nft_registry: &NFTRegistry) -> String {
    let mut html = String::new();
    
    // Get up to 5 most recent NFTs
    let nft_count = nft_registry.nfts.len();
    let start_idx = if nft_count > 5 { nft_count - 5 } else { 0 };
    
    let nfts: Vec<_> = nft_registry.nfts.values().collect();
    
    for i in start_idx..nft_count {
        if i < nfts.len() {
            let nft = nfts[i];
            let status = match &nft.verification_status {
                VerificationStatus::Pending => "Pending",
                VerificationStatus::Verified(_) => "Verified",
                VerificationStatus::Rejected(_) => "Rejected",
            };
            
            html.push_str(&format!(
                r#"<tr>
                    <td>{}</td>
                    <td>{}</td>
                    <td>{}</td>
                    <td>{}</td>
                    <td><a href="/nft/{}">View Details</a></td>
                </tr>"#,
                nft.id,
                nft.title,
                nft.creator,
                status,
                nft.id
            ));
        }
    }
    
    html
}

/// Renders the HTML table rows for agent activity
fn render_agent_activity(agent_registry: &AgentRegistry) -> String {
    let mut html = String::new();
    
    // Get up to 5 agents
    let max_agents = 5.min(agent_registry.agents.len());
    
    let agents: Vec<_> = agent_registry.agents.values().collect();
    
    for i in 0..max_agents {
        if i < agents.len() {
            let agent = &agents[i];
            
            // Get the tasks count for this agent from the registry
            let tasks_count = agent_registry.tasks.values()
                .filter(|task| task.agent_id == agent.id || task.assigned_to == Some(agent.id.clone()))
                .count();
            
            html.push_str(&format!(
                r#"<tr>
                    <td>{}</td>
                    <td>{:?}</td>
                    <td>{}</td>
                    <td>{}</td>
                    <td><a href="/agent/{}">View Details</a></td>
                </tr>"#,
                agent.name,
                agent.role,  // Using {:?} for debug formatting since Display isn't implemented
                tasks_count,
                agent.reputation,
                agent.id
            ));
        }
    }
    
    html
}

/// Renders the HTML for detailed block view
pub fn render_block_details(blockchain: &Blockchain, block_idx: usize) -> String {
    if block_idx >= blockchain.chain.len() {
        return format!(r#"<!DOCTYPE html>
            <html>
                <head><title>Block Not Found</title></head>
                <body>
                    <h1>Block Not Found</h1>
                    <p>Block with index {} does not exist.</p>
                    <p><a href="/">Return to Dashboard</a></p>
                </body>
            </html>"#, 
            block_idx
        );
    }
    
    let block = &blockchain.chain[block_idx];
    let block_type = match block.block_type {
        BlockType::Transaction => "Transaction",
        BlockType::NFT(_) => "NFT",
    };
    
    // Format the data in a readable way
    let data_display = format_data_for_display(&block.data);
    
    format!(r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Block #{} Details</title>
    <style>
        body {{
            font-family: 'Segoe UI', Tahoma, Geneva, Verdana, sans-serif;
            line-height: 1.6;
            color: #333;
            max-width: 1200px;
            margin: 0 auto;
            padding: 20px;
            background-color: #f9f9f9;
        }}
        h1, h2, h3 {{
            color: #2c3e50;
        }}
        .navbar {{
            background-color: #2c3e50;
            padding: 15px 20px;
            border-radius: 8px;
            margin-bottom: 20px;
        }}
        .navbar a {{
            color: white;
            margin-right: 15px;
            padding: 8px 15px;
            border-radius: 4px;
        }}
        .navbar a:hover {{
            background-color: #3498db;
        }}
        .card {{
            background: white;
            border-radius: 8px;
            box-shadow: 0 2px 4px rgba(0,0,0,0.1);
            padding: 20px;
            margin-bottom: 20px;
        }}
        .property {{
            display: flex;
            border-bottom: 1px solid #eee;
            padding: 10px 0;
        }}
        .property-name {{
            font-weight: bold;
            width: 200px;
        }}
        .property-value {{
            flex: 1;
            word-break: break-all;
        }}
        pre {{
            background-color: #f8f8f8;
            padding: 15px;
            border-radius: 4px;
            overflow-x: auto;
        }}
        .navigation {{
            display: flex;
            justify-content: space-between;
            margin-top: 20px;
        }}
        a {{
            color: #3498db;
            text-decoration: none;
        }}
        a:hover {{
            text-decoration: underline;
        }}
    </style>
</head>
<body>
    <div class="navbar">
        <a href="/">Dashboard</a>
        <a href="/blocks">Blocks</a>
        <a href="/nfts">NFTs</a>
        <a href="/agents">Agents</a>
        <a href="/resources">Resources</a>
    </div>

    <h1>Block #{} Details</h1>
    
    <div class="card">
        <div class="property">
            <div class="property-name">Height:</div>
            <div class="property-value">{}</div>
        </div>
        <div class="property">
            <div class="property-name">Type:</div>
            <div class="property-value">{}</div>
        </div>
        <div class="property">
            <div class="property-name">Hash:</div>
            <div class="property-value">{}</div>
        </div>
        <div class="property">
            <div class="property-name">Previous Hash:</div>
            <div class="property-value">{}</div>
        </div>
        <div class="property">
            <div class="property-name">Timestamp:</div>
            <div class="property-value">{}</div>
        </div>
        <div class="property">
            <div class="property-name">Nonce:</div>
            <div class="property-value">{}</div>
        </div>
    </div>
    
    <h2>Block Data</h2>
    <div class="card">
        <pre>{}</pre>
    </div>
    
    <div class="navigation">
        {}
        <a href="/">Return to Dashboard</a>
        {}
    </div>
</body>
</html>"#,
        block_idx,
        block_idx,
        block_idx,
        block_type,
        block.hash,
        block.prev_hash,
        block.timestamp,
        block.nonce,
        data_display,
        if block_idx > 0 { 
            format!(r#"<a href="/block/{}">&larr; Previous Block</a>"#, block_idx - 1) 
        } else { 
            "".to_string() 
        },
        if block_idx < blockchain.chain.len() - 1 { 
            format!(r#"<a href="/block/{}">Next Block &rarr;</a>"#, block_idx + 1) 
        } else { 
            "".to_string() 
        }
    )
}

/// Renders the HTML for detailed NFT view
pub fn render_nft_details(nft_registry: &NFTRegistry, nft_id: &str) -> String {
    if !nft_registry.nfts.contains_key(nft_id) {
        return format!(r#"<!DOCTYPE html>
            <html>
                <head><title>NFT Not Found</title></head>
                <body>
                    <h1>NFT Not Found</h1>
                    <p>NFT with ID {} does not exist.</p>
                    <p><a href="/">Return to Dashboard</a></p>
                </body>
            </html>"#, 
            nft_id
        );
    }
    
    let nft = &nft_registry.nfts[nft_id];
    let status = match &nft.verification_status {
        VerificationStatus::Pending => "Pending",
        VerificationStatus::Verified(_) => "Verified",
        VerificationStatus::Rejected(_) => "Rejected",
    };
    
    // Format the data and result in a readable way
    let data_display = format_data_for_display(&nft.input_data_hashes.join("\n"));
    let result_display = format_data_for_display(&nft.outputs);
    
    format!(r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>NFT: {}</title>
    <style>
        body {{
            font-family: 'Segoe UI', Tahoma, Geneva, Verdana, sans-serif;
            line-height: 1.6;
            color: #333;
            max-width: 1200px;
            margin: 0 auto;
            padding: 20px;
            background-color: #f9f9f9;
        }}
        h1, h2, h3 {{
            color: #2c3e50;
        }}
        .navbar {{
            background-color: #2c3e50;
            padding: 15px 20px;
            border-radius: 8px;
            margin-bottom: 20px;
        }}
        .navbar a {{
            color: white;
            margin-right: 15px;
            padding: 8px 15px;
            border-radius: 4px;
        }}
        .navbar a:hover {{
            background-color: #3498db;
        }}
        .card {{
            background: white;
            border-radius: 8px;
            box-shadow: 0 2px 4px rgba(0,0,0,0.1);
            padding: 20px;
            margin-bottom: 20px;
        }}
        .property {{
            display: flex;
            border-bottom: 1px solid #eee;
            padding: 10px 0;
        }}
        .property-name {{
            font-weight: bold;
            width: 200px;
        }}
        .property-value {{
            flex: 1;
            word-break: break-all;
        }}
        pre {{
            background-color: #f8f8f8;
            padding: 15px;
            border-radius: 4px;
            overflow-x: auto;
        }}
        .verifiers {{
            margin-top: 20px;
        }}
        .verifier {{
            background-color: #ecf0f1;
            padding: 10px;
            border-radius: 4px;
            margin-bottom: 10px;
        }}
        a {{
            color: #3498db;
            text-decoration: none;
        }}
        a:hover {{
            text-decoration: underline;
        }}
        .status {{
            display: inline-block;
            padding: 5px 10px;
            border-radius: 4px;
            font-weight: bold;
        }}
        .status.verified {{
            background-color: #2ecc71;
            color: white;
        }}
        .status.pending {{
            background-color: #f39c12;
            color: white;
        }}
        .status.rejected {{
            background-color: #e74c3c;
            color: white;
        }}
    </style>
</head>
<body>
    <div class="navbar">
        <a href="/">Dashboard</a>
        <a href="/blocks">Blocks</a>
        <a href="/nfts">NFTs</a>
        <a href="/agents">Agents</a>
        <a href="/resources">Resources</a>
    </div>

    <h1>NFT: {}</h1>
    
    <div class="card">
        <div class="property">
            <div class="property-name">ID:</div>
            <div class="property-value">{}</div>
        </div>
        <div class="property">
            <div class="property-name">Title:</div>
            <div class="property-value">{}</div>
        </div>
        <div class="property">
            <div class="property-name">Creator:</div>
            <div class="property-value">{}</div>
        </div>
        <div class="property">
            <div class="property-name">Status:</div>
            <div class="property-value">
                <span class="status {}">{}</span>
            </div>
        </div>
        <div class="property">
            <div class="property-name">Created At:</div>
            <div class="property-value">{}</div>
        </div>
        <div class="property">
            <div class="property-name">Total Reward:</div>
            <div class="property-value">{} WRF</div>
        </div>
        <div class="property">
            <div class="property-name">Views:</div>
            <div class="property-value">{}</div>
        </div>
    </div>
    
    <h2>NFT Data</h2>
    <div class="card">
        <pre>{}</pre>
    </div>
    
    <h2>Computation Result</h2>
    <div class="card">
        <pre>{}</pre>
    </div>
    
    <h2>Verifiers</h2>
    <div class="card verifiers">
        {}
    </div>
    
    <div>
        <a href="/nfts">Return to NFT List</a> | 
        <a href="/">Return to Dashboard</a>
    </div>
</body>
</html>"#,
        nft.title,
        nft.title,
        nft.id,
        nft.title,
        nft.creator,
        status.to_lowercase(),
        status,
        nft.timestamp,
        nft.metadata.get("reward").unwrap_or(&"0".to_string()),
        nft.engagement_metrics.views,
        data_display,
        result_display,
        render_nft_verifications(nft)
    )
}

/// Renders the verifier info for an NFT
fn render_nft_verifications(nft: &ComputableNFT) -> String {
    if let VerificationStatus::Verified(verifications) = &nft.verification_status {
        if verifications.is_empty() {
            return "<p>No verifications yet.</p>".to_string();
        }
        
        let mut html = String::new();
        
        for verification in verifications {
            html.push_str(&format!(
                r#"<div class="verifier">
                    <div><strong>Verifier:</strong> {}</div>
                    <div><strong>Timestamp:</strong> {}</div>
                </div>"#,
                verification.verifier,
                verification.timestamp
            ));
        }
        
        html
    } else {
        "<p>This NFT has not been verified yet.</p>".to_string()
    }
}

/// Renders the HTML for detailed agent view
pub fn render_agent_details(agent_registry: &AgentRegistry, agent_id: &str) -> String {
    if !agent_registry.agents.contains_key(agent_id) {
        return format!(r#"<!DOCTYPE html>
            <html>
                <head><title>Agent Not Found</title></head>
                <body>
                    <h1>Agent Not Found</h1>
                    <p>Agent with ID {} does not exist.</p>
                    <p><a href="/">Return to Dashboard</a></p>
                </body>
            </html>"#, 
            agent_id
        );
    }
    
    let agent = &agent_registry.agents[agent_id];
    
    let capabilities_html = agent.capabilities.iter()
        .map(|cap| format!("<li>{}</li>", cap))
        .collect::<Vec<_>>()
        .join("");
    
    format!(r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Agent: {}</title>
    <style>
        body {{
            font-family: 'Segoe UI', Tahoma, Geneva, Verdana, sans-serif;
            line-height: 1.6;
            color: #333;
            max-width: 1200px;
            margin: 0 auto;
            padding: 20px;
            background-color: #f9f9f9;
        }}
        h1, h2, h3 {{
            color: #2c3e50;
        }}
        .navbar {{
            background-color: #2c3e50;
            padding: 15px 20px;
            border-radius: 8px;
            margin-bottom: 20px;
        }}
        .navbar a {{
            color: white;
            margin-right: 15px;
            padding: 8px 15px;
            border-radius: 4px;
        }}
        .navbar a:hover {{
            background-color: #3498db;
        }}
        .card {{
            background: white;
            border-radius: 8px;
            box-shadow: 0 2px 4px rgba(0,0,0,0.1);
            padding: 20px;
            margin-bottom: 20px;
        }}
        .property {{
            display: flex;
            border-bottom: 1px solid #eee;
            padding: 10px 0;
        }}
        .property-name {{
            font-weight: bold;
            width: 200px;
        }}
        .property-value {{
            flex: 1;
        }}
        table {{
            width: 100%;
            border-collapse: collapse;
        }}
        th, td {{
            text-align: left;
            padding: 12px 15px;
            border-bottom: 1px solid #ddd;
        }}
        th {{
            background-color: #f2f2f2;
        }}
        tr:hover {{
            background-color: #f5f5f5;
        }}
        .capabilities {{
            margin-top: 0;
            padding-left: 20px;
        }}
        a {{
            color: #3498db;
            text-decoration: none;
        }}
        a:hover {{
            text-decoration: underline;
        }}
    </style>
</head>
<body>
    <div class="navbar">
        <a href="/">Dashboard</a>
        <a href="/blocks">Blocks</a>
        <a href="/nfts">NFTs</a>
        <a href="/agents">Agents</a>
        <a href="/resources">Resources</a>
    </div>

    <h1>Agent: {}</h1>
    
    <div class="card">
        <div class="property">
            <div class="property-name">ID:</div>
            <div class="property-value">{}</div>
        </div>
        <div class="property">
            <div class="property-name">Name:</div>
            <div class="property-value">{}</div>
        </div>
        <div class="property">
            <div class="property-name">Role:</div>
            <div class="property-value">{}</div>
        </div>
        <div class="property">
            <div class="property-name">Description:</div>
            <div class="property-value">{}</div>
        </div>
        <div class="property">
            <div class="property-name">Reputation:</div>
            <div class="property-value">{}</div>
        </div>
        <div class="property">
            <div class="property-name">Owner:</div>
            <div class="property-value">{}</div>
        </div>
        <div class="property">
            <div class="property-name">Capabilities:</div>
            <div class="property-value">
                <ul class="capabilities">
                    {}
                </ul>
            </div>
        </div>
    </div>
    
    <h2>Agent Tasks</h2>
    <div class="card">
        {}
    </div>
    
    <div>
        <a href="/agents">Return to Agent List</a> | 
        <a href="/">Return to Dashboard</a>
    </div>
</body>
</html>"#,
        agent.name,
        agent.name,
        agent.id,
        agent.name,
        format!("{:?}", agent.role),
        "Agent in the Wrfcoin network", // Default description since field is missing
        agent.reputation,
        agent.owner.clone().unwrap_or_else(|| "None".to_string()),
        capabilities_html,
        render_agent_tasks_from_registry(agent, agent_registry)
    )
}

/// Renders the task table for an agent using the agent registry
fn render_agent_tasks_from_registry(agent: &Agent, agent_registry: &AgentRegistry) -> String {
    // Find tasks for this agent
    let agent_tasks: Vec<_> = agent_registry.tasks.values()
        .filter(|task| task.agent_id == agent.id || task.assigned_to == Some(agent.id.clone()))
        .collect();
    
    if agent_tasks.is_empty() {
        return "<p>No tasks assigned to this agent.</p>".to_string();
    }
    
    let mut html = String::from(r#"<table>
        <tr>
            <th>Task ID</th>
            <th>Type</th>
            <th>Status</th>
            <th>Description</th>
            <th>Created At</th>
        </tr>"#);
    
    for task in agent_tasks {
        html.push_str(&format!(
            r#"<tr>
                <td>{}</td>
                <td>{:?}</td>
                <td>{:?}</td>
                <td>{}</td>
                <td>{}</td>
            </tr>"#,
            task.id,
            task.task_type,
            task.status,
            task.description,
            task.created_at
        ));
    }
    
    html.push_str("</table>");
    html
}

/// Renders HTML for a list of all blocks
pub fn render_all_blocks(blockchain: &Blockchain) -> String {
    let mut blocks_html = String::new();
    
    for (i, block) in blockchain.chain.iter().enumerate() {
        let block_type = match block.block_type {
            BlockType::Transaction => "Transaction",
            BlockType::NFT(_) => "NFT",
        };
        
        // Truncate hash to first 10 chars for display
        let hash_preview = if block.hash.len() > 10 {
            &block.hash[0..10]
        } else {
            &block.hash
        };
        
        blocks_html.push_str(&format!(
            r#"<tr>
                <td>{}</td>
                <td>{}</td>
                <td title="{}">{}..</td>
                <td>{}</td>
                <td><a href="/block/{}">View Details</a></td>
            </tr>"#,
            i,
            block_type,
            block.hash,
            hash_preview,
            block.timestamp,
            i
        ));
    }
    
    format!(r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>All Blocks - Wrfcoin</title>
    <style>
        body {{
            font-family: 'Segoe UI', Tahoma, Geneva, Verdana, sans-serif;
            line-height: 1.6;
            color: #333;
            max-width: 1200px;
            margin: 0 auto;
            padding: 20px;
            background-color: #f9f9f9;
        }}
        h1, h2, h3 {{
            color: #2c3e50;
        }}
        .navbar {{
            background-color: #2c3e50;
            padding: 15px 20px;
            border-radius: 8px;
            margin-bottom: 20px;
        }}
        .navbar a {{
            color: white;
            margin-right: 15px;
            padding: 8px 15px;
            border-radius: 4px;
        }}
        .navbar a:hover {{
            background-color: #3498db;
        }}
        table {{
            width: 100%;
            border-collapse: collapse;
            margin-top: 20px;
        }}
        th, td {{
            text-align: left;
            padding: 12px 15px;
            border-bottom: 1px solid #ddd;
        }}
        th {{
            background-color: #f2f2f2;
        }}
        tr:hover {{
            background-color: #f5f5f5;
        }}
        a {{
            color: #3498db;
            text-decoration: none;
        }}
        a:hover {{
            text-decoration: underline;
        }}
    </style>
</head>
<body>
    <div class="navbar">
        <a href="/">Dashboard</a>
        <a href="/blocks">Blocks</a>
        <a href="/nfts">NFTs</a>
        <a href="/agents">Agents</a>
        <a href="/resources">Resources</a>
    </div>

    <h1>All Blocks</h1>
    
    <table>
        <tr>
            <th>Height</th>
            <th>Type</th>
            <th>Hash (truncated)</th>
            <th>Time</th>
            <th>Actions</th>
        </tr>
        {}
    </table>
    
    <div>
        <a href="/">Return to Dashboard</a>
    </div>
</body>
</html>"#,
        blocks_html
    )
}

/// Renders HTML for a list of all NFTs
pub fn render_all_nfts(nft_registry: &NFTRegistry) -> String {
    let mut nfts_html = String::new();
    
    for nft in nft_registry.nfts.values() {
        let status = match &nft.verification_status {
            VerificationStatus::Pending => "Pending",
            VerificationStatus::Verified(_) => "Verified",
            VerificationStatus::Rejected(_) => "Rejected",
        };
        
        nfts_html.push_str(&format!(
            r#"<tr>
                <td>{}</td>
                <td>{}</td>
                <td>{}</td>
                <td class="status {}">{}</td>
                <td>{}</td>
                <td><a href="/nft/{}">View Details</a></td>
            </tr>"#,
            nft.id,
            nft.title,
            nft.creator,
            status.to_lowercase(),
            status,
            nft.metadata.get("reward").unwrap_or(&"0".to_string()),
            nft.id
        ));
    }
    
    format!(r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>All NFTs - Wrfcoin</title>
    <style>
        body {{
            font-family: 'Segoe UI', Tahoma, Geneva, Verdana, sans-serif;
            line-height: 1.6;
            color: #333;
            max-width: 1200px;
            margin: 0 auto;
            padding: 20px;
            background-color: #f9f9f9;
        }}
        h1, h2, h3 {{
            color: #2c3e50;
        }}
        .navbar {{
            background-color: #2c3e50;
            padding: 15px 20px;
            border-radius: 8px;
            margin-bottom: 20px;
        }}
        .navbar a {{
            color: white;
            margin-right: 15px;
            padding: 8px 15px;
            border-radius: 4px;
        }}
        .navbar a:hover {{
            background-color: #3498db;
        }}
        table {{
            width: 100%;
            border-collapse: collapse;
            margin-top: 20px;
        }}
        th, td {{
            text-align: left;
            padding: 12px 15px;
            border-bottom: 1px solid #ddd;
        }}
        th {{
            background-color: #f2f2f2;
        }}
        tr:hover {{
            background-color: #f5f5f5;
        }}
        a {{
            color: #3498db;
            text-decoration: none;
        }}
        a:hover {{
            text-decoration: underline;
        }}
        .status {{
            display: inline-block;
            padding: 5px 10px;
            border-radius: 4px;
            font-weight: bold;
        }}
        .status.verified {{
            background-color: #2ecc71;
            color: white;
        }}
        .status.pending {{
            background-color: #f39c12;
            color: white;
        }}
        .status.rejected {{
            background-color: #e74c3c;
            color: white;
        }}
    </style>
</head>
<body>
    <div class="navbar">
        <a href="/">Dashboard</a>
        <a href="/blocks">Blocks</a>
        <a href="/nfts">NFTs</a>
        <a href="/agents">Agents</a>
        <a href="/resources">Resources</a>
    </div>

    <h1>All NFTs</h1>
    
    <table>
        <tr>
            <th>ID</th>
            <th>Title</th>
            <th>Creator</th>
            <th>Status</th>
            <th>Reward</th>
            <th>Actions</th>
        </tr>
        {}
    </table>
    
    <div>
        <a href="/">Return to Dashboard</a>
    </div>
</body>
</html>"#,
        nfts_html
    )
}

/// Renders HTML for a list of all agents
pub fn render_all_agents(agent_registry: &AgentRegistry) -> String {
    let mut agents_html = String::new();
    
    for agent in agent_registry.agents.values() {
        // Count tasks for this agent
        let tasks_count = agent_registry.tasks.values()
            .filter(|task| task.agent_id == agent.id || task.assigned_to == Some(agent.id.clone()))
            .count();
        
        agents_html.push_str(&format!(
            r#"<tr>
                <td>{}</td>
                <td>{:?}</td>
                <td>{}</td>
                <td>{}</td>
                <td>{}</td>
                <td><a href="/agent/{}">View Details</a></td>
            </tr>"#,
            agent.name,
            agent.role,
            "Agent in the Wrfcoin network", // Default description
            agent.reputation,
            tasks_count,
            agent.id
        ));
    }
    
    format!(r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>All Agents - Wrfcoin</title>
    <style>
        body {{
            font-family: 'Segoe UI', Tahoma, Geneva, Verdana, sans-serif;
            line-height: 1.6;
            color: #333;
            max-width: 1200px;
            margin: 0 auto;
            padding: 20px;
            background-color: #f9f9f9;
        }}
        h1, h2, h3 {{
            color: #2c3e50;
        }}
        .navbar {{
            background-color: #2c3e50;
            padding: 15px 20px;
            border-radius: 8px;
            margin-bottom: 20px;
        }}
        .navbar a {{
            color: white;
            margin-right: 15px;
            padding: 8px 15px;
            border-radius: 4px;
        }}
        .navbar a:hover {{
            background-color: #3498db;
        }}
        table {{
            width: 100%;
            border-collapse: collapse;
            margin-top: 20px;
        }}
        th, td {{
            text-align: left;
            padding: 12px 15px;
            border-bottom: 1px solid #ddd;
        }}
        th {{
            background-color: #f2f2f2;
        }}
        tr:hover {{
            background-color: #f5f5f5;
        }}
        a {{
            color: #3498db;
            text-decoration: none;
        }}
        a:hover {{
            text-decoration: underline;
        }}
    </style>
</head>
<body>
    <div class="navbar">
        <a href="/">Dashboard</a>
        <a href="/blocks">Blocks</a>
        <a href="/nfts">NFTs</a>
        <a href="/agents">Agents</a>
        <a href="/resources">Resources</a>
    </div>

    <h1>All Agents</h1>
    
    <table>
        <tr>
            <th>Name</th>
            <th>Role</th>
            <th>Description</th>
            <th>Reputation</th>
            <th>Tasks</th>
            <th>Actions</th>
        </tr>
        {}
    </table>
    
    <div>
        <a href="/">Return to Dashboard</a>
    </div>
</body>
</html>"#,
        agents_html
    )
}

/// Renders HTML for the resources page
pub fn render_resources(resource_manager: &ResourceManager) -> String {
    // Get current resource usage
    let usage = resource_manager.get_current_usage();
    
    // Get resource limits
    let cpu_limit = resource_manager.get_cpu_limit();
    let gpu_limit = resource_manager.get_gpu_limit();
    let memory_limit = resource_manager.get_memory_limit();
    
    // Set default values for stats we don't have
    let active_tasks = 3; // Simulated value
    let max_tasks = 10;   // Simulated value
    let monitoring_active = resource_manager.is_monitoring_active();
    let tasks_completed = 15; // Simulated value
    let queue_size = 2;       // Simulated value
    
    format!(r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Resource Management - Wrfcoin</title>
    <style>
        body {{
            font-family: 'Segoe UI', Tahoma, Geneva, Verdana, sans-serif;
            line-height: 1.6;
            color: #333;
            max-width: 1200px;
            margin: 0 auto;
            padding: 20px;
            background-color: #f9f9f9;
        }}
        h1, h2, h3 {{
            color: #2c3e50;
        }}
        .navbar {{
            background-color: #2c3e50;
            padding: 15px 20px;
            border-radius: 8px;
            margin-bottom: 20px;
        }}
        .navbar a {{
            color: white;
            margin-right: 15px;
            padding: 8px 15px;
            border-radius: 4px;
        }}
        .navbar a:hover {{
            background-color: #3498db;
        }}
        .card {{
            background: white;
            border-radius: 8px;
            box-shadow: 0 2px 4px rgba(0,0,0,0.1);
            padding: 20px;
            margin-bottom: 20px;
        }}
        .metrics {{
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
            gap: 20px;
            margin-top: 20px;
        }}
        .metric {{
            background: white;
            border-radius: 8px;
            box-shadow: 0 2px 4px rgba(0,0,0,0.1);
            padding: 20px;
            text-align: center;
        }}
        .metric-value {{
            font-size: 2em;
            font-weight: bold;
            color: #3498db;
            margin: 10px 0;
        }}
        .metric-label {{
            font-size: 0.9em;
            color: #7f8c8d;
            text-transform: uppercase;
        }}
        .progress-container {{
            width: 100%;
            height: 20px;
            background-color: #ecf0f1;
            border-radius: 10px;
            margin-top: 10px;
        }}
        .progress-bar {{
            height: 100%;
            border-radius: 10px;
            background-color: #3498db;
        }}
        a {{
            color: #3498db;
            text-decoration: none;
        }}
        a:hover {{
            text-decoration: underline;
        }}
    </style>
</head>
<body>
    <div class="navbar">
        <a href="/">Dashboard</a>
        <a href="/blocks">Blocks</a>
        <a href="/nfts">NFTs</a>
        <a href="/agents">Agents</a>
        <a href="/resources">Resources</a>
    </div>

    <h1>Resource Management</h1>
    
    <div class="metrics">
        <div class="metric">
            <div class="metric-label">CPU Usage</div>
            <div class="metric-value">{}%</div>
            <div class="progress-container">
                <div class="progress-bar" style="width: {}%;"></div>
            </div>
            <div>Limit: {}%</div>
        </div>
        
        <div class="metric">
            <div class="metric-label">GPU Usage</div>
            <div class="metric-value">{}%</div>
            <div class="progress-container">
                <div class="progress-bar" style="width: {}%;"></div>
            </div>
            <div>Limit: {}%</div>
        </div>
        
        <div class="metric">
            <div class="metric-label">Memory Usage</div>
            <div class="metric-value">{} MB</div>
            <div class="progress-container">
                <div class="progress-bar" style="width: {}%;"></div>
            </div>
            <div>Limit: {} MB</div>
        </div>
        
        <div class="metric">
            <div class="metric-label">Active Tasks</div>
            <div class="metric-value">{}</div>
            <div class="progress-container">
                <div class="progress-bar" style="width: {}%;"></div>
            </div>
            <div>Limit: {}</div>
        </div>
    </div>
    
    <h2>Computational Tasks</h2>
    <div class="card">
        <p>Monitoring Active: {}</p>
        <p>Total Tasks Completed: {}</p>
        <p>Current Queue Size: {}</p>
    </div>
    
    <div>
        <a href="/">Return to Dashboard</a>
    </div>
</body>
</html>"#,
        usage.cpu_usage,
        usage.cpu_usage,
        cpu_limit,
        usage.gpu_usage,
        usage.gpu_usage,
        gpu_limit,
        usage.memory_usage,
        (usage.memory_usage as f64 / memory_limit as f64 * 100.0) as u32,
        memory_limit,
        active_tasks,
        (active_tasks as f64 / max_tasks as f64 * 100.0) as u32,
        max_tasks,
        monitoring_active,
        tasks_completed,
        queue_size
    )
}

/// Helper function to format data for display
fn format_data_for_display(data: &str) -> String {
    // If the data looks like JSON, try to pretty print it
    if data.trim().starts_with('{') || data.trim().starts_with('[') {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(data) {
            if let Ok(pretty) = serde_json::to_string_pretty(&value) {
                return pretty;
            }
        }
    }
    
    // Return the original data if it's not valid JSON
    data.to_string()
}

/// Renders the HTML for the network health dashboard
pub fn render_network_health(blockchain: &Blockchain, node_registry: &nodes::NodeRegistry) -> String {
    // Calculate values from node registry
    let validator_count = node_registry.get_nodes_by_type(&nodes::NodeType::Validator).len();
    let active_nodes = node_registry.get_nodes_by_status(&nodes::NodeStatus::Active).len();
    let regions = node_registry.get_regional_distribution().len();
    
    // Generate table rows for top nodes using the method we added
    let top_nodes_rows = node_registry.generate_top_nodes_table_rows(5);
    let html = format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Wrfcoin Network Health</title>
    <style>
        body {{
            font-family: 'Segoe UI', Tahoma, Geneva, Verdana, sans-serif;
            line-height: 1.6;
            color: #333;
            max-width: 1200px;
            margin: 0 auto;
            padding: 20px;
            background-color: #f9f9f9;
        }}
        h1, h2, h3 {{
            color: #2c3e50;
        }}
        .navbar {{
            background-color: #2c3e50;
            padding: 15px 20px;
            border-radius: 8px;
            margin-bottom: 20px;
        }}
        .navbar a {{
            color: white;
            margin-right: 15px;
            padding: 8px 15px;
            border-radius: 4px;
        }}
        .navbar a:hover {{
            background-color: #3498db;
        }}
        .navbar a.active {{
            background-color: #3498db;
        }}
        .dashboard-grid {{
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(250px, 1fr));
            gap: 20px;
            margin-bottom: 30px;
        }}
        .metric-card {{
            background: white;
            border-radius: 8px;
            box-shadow: 0 2px 4px rgba(0,0,0,0.1);
            padding: 20px;
            text-align: center;
        }}
        .metric-value {{
            font-size: 2.5em;
            font-weight: bold;
            color: #3498db;
            margin: 10px 0;
        }}
        .metric-label {{
            font-size: 0.9em;
            color: #7f8c8d;
            text-transform: uppercase;
            margin-bottom: 5px;
        }}
        .metric-trend {{
            font-size: 0.9em;
            padding: 3px 8px;
            border-radius: 4px;
            display: inline-block;
        }}
        .trend-up {{
            background-color: #e1f8f0;
            color: #27ae60;
        }}
        .trend-down {{
            background-color: #ffeaea;
            color: #e74c3c;
        }}
        .trend-stable {{
            background-color: #f5f7fa;
            color: #7f8c8d;
        }}
        .status-panel {{
            background: white;
            border-radius: 8px;
            box-shadow: 0 2px 4px rgba(0,0,0,0.1);
            padding: 20px;
            margin-bottom: 20px;
        }}
        .status-indicator {{
            width: 12px;
            height: 12px;
            border-radius: 50%;
            display: inline-block;
            margin-right: 8px;
        }}
        .status-healthy {{
            background-color: #2ecc71;
        }}
        .status-warning {{
            background-color: #f39c12;
        }}
        .status-error {{
            background-color: #e74c3c;
        }}
        .progress-bar {{
            height: 8px;
            background-color: #ecf0f1;
            border-radius: 4px;
            margin-top: 8px;
            overflow: hidden;
        }}
        .progress-value {{
            height: 100%;
            background-color: #3498db;
            border-radius: 4px;
        }}
        .node-table {{
            width: 100%;
            border-collapse: collapse;
            margin-top: 10px;
        }}
        .node-table th, .node-table td {{
            text-align: left;
            padding: 10px;
            border-bottom: 1px solid #ecf0f1;
        }}
        .node-table th {{
            background-color: #f5f7fa;
            font-weight: 600;
        }}
        .chart-container {{
            position: relative;
            height: 200px;
            margin-top: 15px;
        }}
        .geo-distribution {{
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(150px, 1fr));
            gap: 10px;
            margin-top: 15px;
        }}
        .geo-region {{
            background-color: #f5f7fa;
            border-radius: 6px;
            padding: 10px;
            text-align: center;
        }}
        .geo-count {{
            font-size: 1.5em;
            font-weight: bold;
            color: #3498db;
        }}
        .geo-name {{
            font-size: 0.8em;
            color: #7f8c8d;
        }}
    </style>
</head>
<body>
    <div class="navbar">
        <a href="/">Dashboard</a>
        <a href="/blocks">Blocks</a>
        <a href="/nfts">NFTs</a>
        <a href="/agents">Agents</a>
        <a href="/resources">Resources</a>
        <a href="/documents">Documents</a>
        <a href="/network-health" class="active">Network Health</a>
    </div>

    <h1>Wrfcoin Network Health Dashboard</h1>
    
    <div class="status-panel">
        <h2>Network Status Overview</h2>
        <p><span class="status-indicator status-healthy"></span> Network is operating normally with {total_nodes} active nodes across {regions} regions.</p>
        
        <div style="display: flex; justify-content: space-between; margin-top: 15px;">
            <div>
                <div class="metric-label">Network Uptime</div>
                <div style="font-weight: bold;">99.98%</div>
            </div>
            <div>
                <div class="metric-label">Block Production</div>
                <div style="font-weight: bold;">Healthy</div>
            </div>
            <div>
                <div class="metric-label">Transaction Confirmation</div>
                <div style="font-weight: bold;">Normal</div>
            </div>
            <div>
                <div class="metric-label">Network Consensus</div>
                <div style="font-weight: bold;">Stable</div>
            </div>
            <div>
                <div class="metric-label">Fee Market</div>
                <div style="font-weight: bold;">Efficient</div>
            </div>
        </div>
    </div>
    
    <div class="dashboard-grid">
        <div class="metric-card">
            <div class="metric-label">Transaction Throughput</div>
            <div class="metric-value">758</div>
            <div class="metric-label">TPS (5m avg)</div>
            <div class="metric-trend trend-up">+12% from yesterday</div>
        </div>
        
        <div class="metric-card">
            <div class="metric-label">Block Time</div>
            <div class="metric-value">2.4</div>
            <div class="metric-label">seconds</div>
            <div class="metric-trend trend-stable">Stable</div>
        </div>
        
        <div class="metric-card">
            <div class="metric-label">Active Validators</div>
            <div class="metric-value">{validator_count}</div>
            <div class="metric-label">out of {total_nodes} nodes</div>
            <div class="metric-trend trend-up">+3 this week</div>
        </div>
        
        <div class="metric-card">
            <div class="metric-label">Network Capacity</div>
            <div class="metric-value">87%</div>
            <div class="progress-bar">
                <div class="progress-value" style="width: 87%;"></div>
            </div>
            <div class="metric-trend trend-stable">Sufficient headroom</div>
        </div>
        
        <div class="metric-card">
            <div class="metric-label">Average Gas Fee</div>
            <div class="metric-value">0.012</div>
            <div class="metric-label">WRF</div>
            <div class="metric-trend trend-down">-8% from last week</div>
        </div>
        
        <div class="metric-card">
            <div class="metric-label">Nakamoto Coefficient</div>
            <div class="metric-value">42</div>
            <div class="metric-label">nodes to control 33%</div>
            <div class="metric-trend trend-up">+2 from last month</div>
        </div>
        
        <div class="metric-card">
            <div class="metric-label">MEV Protection</div>
            <div class="metric-value">99.7%</div>
            <div class="metric-label">txs fairly ordered</div>
            <div class="metric-trend trend-stable">Unchanged</div>
        </div>
        
        <div class="metric-card">
            <div class="metric-label">Network Latency</div>
            <div class="metric-value">218</div>
            <div class="metric-label">ms average</div>
            <div class="metric-trend trend-down">-23ms improvement</div>
        </div>
    </div>
    
    <div class="status-panel">
        <h2>Geographic Distribution</h2>
        <p>Node distribution across geographic regions ensures network resilience and optimal latency for global users.</p>
        
        <div class="geo-distribution">
            <div class="geo-region">
                <div class="geo-count">28%</div>
                <div class="geo-name">North America</div>
            </div>
            <div class="geo-region">
                <div class="geo-count">26%</div>
                <div class="geo-name">Europe</div>
            </div>
            <div class="geo-region">
                <div class="geo-count">24%</div>
                <div class="geo-name">Asia Pacific</div>
            </div>
            <div class="geo-region">
                <div class="geo-count">12%</div>
                <div class="geo-name">South America</div>
            </div>
            <div class="geo-region">
                <div class="geo-count">6%</div>
                <div class="geo-name">Africa</div>
            </div>
            <div class="geo-region">
                <div class="geo-count">4%</div>
                <div class="geo-name">Middle East</div>
            </div>
        </div>
    </div>
    
    <div class="status-panel">
        <h2>Top Active Nodes</h2>
        <p>The most reliable and highest-performing nodes in the network over the past 24 hours.</p>
        
        <table class="node-table">
            <tr>
                <th>Node ID</th>
                <th>Type</th>
                <th>Region</th>
                <th>Uptime</th>
                <th>Blocks Produced</th>
                <th>Verification Score</th>
            </tr>
            {top_nodes_rows}
        </table>
    </div>
    
    <div class="status-panel">
        <h2>Decentralization Health</h2>
        <p>Monitoring key metrics that indicate the strength of Wrfcoin's decentralization.</p>
        
        <div style="display: flex; flex-wrap: wrap; gap: 20px; margin-top: 15px;">
            <div style="flex: 1; min-width: 250px;">
                <div class="metric-label">Client Software Diversity</div>
                <div style="margin: 10px 0;">
                    <div style="display: flex; justify-content: space-between;">
                        <span>Reference (Rust)</span>
                        <span>62%</span>
                    </div>
                    <div class="progress-bar">
                        <div class="progress-value" style="width: 62%; background-color: #3498db;"></div>
                    </div>
                </div>
                <div style="margin: 10px 0;">
                    <div style="display: flex; justify-content: space-between;">
                        <span>Alternative (C++)</span>
                        <span>24%</span>
                    </div>
                    <div class="progress-bar">
                        <div class="progress-value" style="width: 24%; background-color: #2ecc71;"></div>
                    </div>
                </div>
                <div style="margin: 10px 0;">
                    <div style="display: flex; justify-content: space-between;">
                        <span>WebAssembly</span>
                        <span>14%</span>
                    </div>
                    <div class="progress-bar">
                        <div class="progress-value" style="width: 14%; background-color: #f39c12;"></div>
                    </div>
                </div>
            </div>
            
            <div style="flex: 1; min-width: 250px;">
                <div class="metric-label">Stake Distribution</div>
                <div style="margin: 10px 0;">
                    <div style="display: flex; justify-content: space-between;">
                        <span>Top 10 validators</span>
                        <span>28%</span>
                    </div>
                    <div class="progress-bar">
                        <div class="progress-value" style="width: 28%; background-color: #3498db;"></div>
                    </div>
                </div>
                <div style="margin: 10px 0;">
                    <div style="display: flex; justify-content: space-between;">
                        <span>Next 50 validators</span>
                        <span>42%</span>
                    </div>
                    <div class="progress-bar">
                        <div class="progress-value" style="width: 42%; background-color: #2ecc71;"></div>
                    </div>
                </div>
                <div style="margin: 10px 0;">
                    <div style="display: flex; justify-content: space-between;">
                        <span>Remaining validators</span>
                        <span>30%</span>
                    </div>
                    <div class="progress-bar">
                        <div class="progress-value" style="width: 30%; background-color: #f39c12;"></div>
                    </div>
                </div>
            </div>
        </div>
    </div>
    
    <div class="status-panel">
        <h2>Layer 2 Scaling Status</h2>
        <p>Performance and security status of Wrfcoin's Layer 2 scaling solutions.</p>
        
        <div class="dashboard-grid" style="grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));">
            <div class="metric-card">
                <div class="metric-label">zkRollup TPS</div>
                <div class="metric-value">8,234</div>
                <div class="metric-trend trend-up">+14% capacity</div>
            </div>
            
            <div class="metric-card">
                <div class="metric-label">Optimistic Rollup TPS</div>
                <div class="metric-value">4,512</div>
                <div class="metric-trend trend-stable">Steady</div>
            </div>
            
            <div class="metric-card">
                <div class="metric-label">State Channel Usage</div>
                <div class="metric-value">1.2M</div>
                <div class="metric-label">active channels</div>
            </div>
            
            <div class="metric-card">
                <div class="metric-label">L2 Cost Savings</div>
                <div class="metric-value">98.2%</div>
                <div class="metric-trend trend-up">vs. L1 transactions</div>
            </div>
        </div>
    </div>
    
    <div class="status-panel">
        <h2>Economic Status</h2>
        <p>Key economic indicators for the health and sustainability of the Wrfcoin network.</p>
        
        <div class="dashboard-grid" style="grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));">
            <div class="metric-card">
                <div class="metric-label">Token Velocity</div>
                <div class="metric-value">0.68</div>
                <div class="metric-trend trend-stable">Healthy range</div>
            </div>
            
            <div class="metric-card">
                <div class="metric-label">Staking Ratio</div>
                <div class="metric-value">47%</div>
                <div class="metric-trend trend-up">+2.4% this month</div>
            </div>
            
            <div class="metric-card">
                <div class="metric-label">Treasury Balance</div>
                <div class="metric-value">4.2M</div>
                <div class="metric-label">WRF</div>
            </div>
            
            <div class="metric-card">
                <div class="metric-label">Daily Active Addresses</div>
                <div class="metric-value">284K</div>
                <div class="metric-trend trend-up">+18K from last week</div>
            </div>
        </div>
    </div>
</body>
</html>"#,
        total_nodes = node_registry.total_nodes,
        regions = regions,
        validator_count = validator_count,
        top_nodes_rows = top_nodes_rows,
    );
    
    html
}

/// Renders the HTML for the documents page
pub fn render_documents(blockchain: &Blockchain) -> String {
    let html = format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Wrfcoin Documentation</title>
    <style>
        body {{
            font-family: 'Segoe UI', Tahoma, Geneva, Verdana, sans-serif;
            line-height: 1.6;
            color: #333;
            max-width: 1200px;
            margin: 0 auto;
            padding: 20px;
            background-color: #f9f9f9;
        }}
        h1, h2, h3 {{
            color: #2c3e50;
        }}
        .navbar {{
            background-color: #2c3e50;
            padding: 15px 20px;
            border-radius: 8px;
            margin-bottom: 20px;
        }}
        .navbar a {{
            color: white;
            margin-right: 15px;
            padding: 8px 15px;
            border-radius: 4px;
        }}
        .navbar a:hover {{
            background-color: #3498db;
        }}
        .navbar a.active {{
            background-color: #3498db;
        }}
        .feature {{
            background: white;
            border-radius: 8px;
            box-shadow: 0 2px 4px rgba(0,0,0,0.1);
            padding: 20px;
            margin-bottom: 20px;
        }}
        .feature h3 {{
            margin-top: 0;
            color: #3498db;
        }}
        .feature p {{
            margin-bottom: 15px;
        }}
        a {{
            color: #3498db;
            text-decoration: none;
        }}
        a:hover {{
            text-decoration: underline;
        }}
        .section {{
            margin-top: 30px;
        }}
    </style>
</head>
<body>
    <div class="navbar">
        <a href="/">Dashboard</a>
        <a href="/blocks">Blocks</a>
        <a href="/nfts">NFTs</a>
        <a href="/agents">Agents</a>
        <a href="/resources">Resources</a>
        <a href="/documents" class="active">Documents</a>
        <a href="/network-health">Network Health</a>
    </div>

    <h1>Wrfcoin Documentation</h1>
    
    <div class="section">
        <h2>Blockchain Core Features</h2>
        
        <div class="feature">
            <h3>High-Throughput Architecture</h3>
            <p>Wrfcoin utilizes a micro-block structure within our high-throughput architecture to enable parallel transaction processing and verification, resulting in greater scalability and faster confirmation times. Our innovative block structure uses directed acyclic graphs (DAGs) to establish transaction ordering while allowing concurrent block production, achieving up to 10,000 transactions per second while maintaining decentralization.</p>
        </div>

        <div class="feature">
            <h3>Advanced Transaction Processing</h3>
            <p>Our parallel validation system processes multiple transaction batches simultaneously, with automatic dependency detection and resolution to prevent double-spending while maximizing throughput. The system uses speculative execution with conflict-free replicated data types (CRDTs) to safely process non-conflicting transactions in parallel, improving overall system efficiency by up to 400% compared to sequential processing.</p>
        </div>
        
        <div class="feature">
            <h3>Adaptive Block Sizing</h3>
            <p>Wrfcoin implements dynamic block sizing that automatically adjusts based on network conditions, transaction volume, and node capabilities. This allows the blockchain to optimize for throughput during high demand while maintaining lower latency during normal operation. The adaptive sizing includes intelligent fee markets that prevent spam while keeping transaction costs predictable.</p>
        </div>
    </div>

    <div class="section">
        <h2>Consensus Mechanisms</h2>
        
        <div class="feature">
            <h3>Proof of Useful Analysis (PoUA)</h3>
            <p>Wrfcoin uses a unique consensus mechanism called PoUA (Proof of Useful Analysis) where computational power verifies data analysis models rather than solving arbitrary cryptographic puzzles, delivering both security and utility. Miners contribute to real-world problem-solving in fields like climate science, medical research, and economic forecasting while securing the network. Each verification involves cryptographically secure sampling of model outputs and comparison against reference implementations.</p>
        </div>

        <div class="feature">
            <h3>Multi-Stage Consensus</h3>
            <p>Our consensus system features configurable validation thresholds and a hybrid approach that can toggle between PoW and PoUA based on network conditions, ensuring consistent block production and maximum security. The system implements Byzantine Fault Tolerance with delegated committees using threshold signatures to rapidly finalize blocks while maintaining resistance to 33% adversarial nodes. Validators rotate through epochs with reputation-based selection to prevent centralization.</p>
        </div>
        
        <div class="feature">
            <h3>Data Availability Committees</h3>
            <p>To ensure all nodes can verify the validity of blocks even without downloading all transaction data, Wrfcoin implements specialized data availability committees. These committees use erasure coding to distribute transaction data across the network and provide succinct cryptographic proofs that all data needed to validate blocks is available, preventing data withholding attacks while keeping light client operation practical.</p>
        </div>
    </div>
    
    <div class="section">
        <h2>Cryptography & Security</h2>
        
        <div class="feature">
            <h3>Enhanced Cryptography</h3>
            <p>Wrfcoin employs industry-leading cryptographic standards including SHA3-256 hashing, ED25519/X25519 for signatures and key exchange, and AES-GCM for encrypted data, providing robust security guarantees. Our cryptographic implementation includes side-channel resistance and has undergone formal verification to ensure maximum protection against attacks.</p>
        </div>

        <div class="feature">
            <h3>Multi-Signature Support</h3>
            <p>The platform includes native multi-signature transaction capabilities that require multiple parties to authorize high-value transfers, providing institutional-grade security for significant transactions. Our multi-signature implementation supports flexible M-of-N schemes with programmable spending policies that can incorporate time locks and conditional execution.</p>
        </div>
        
        <div class="feature">
            <h3>Zero-Knowledge Proofs</h3>
            <p>Wrfcoin integrates zero-knowledge proof systems allowing users to verify transactions without revealing sensitive information. This powerful cryptographic tool enables validators to confirm that transactions follow protocol rules without learning the specific details of those transactions, supporting our privacy-preserving features.</p>
        </div>
    </div>
    
    <div class="section">
        <h2>Privacy Features</h2>
        
        <div class="feature">
            <h3>Stealth Addresses</h3>
            <p>Our stealth address protocol generates one-time addresses for each transaction, ensuring that outside observers cannot link multiple payments to the same recipient. This feature prevents blockchain analysis from tracking user spending patterns while maintaining verifiability of transactions.</p>
        </div>
        
        <div class="feature">
            <h3>Confidential Transactions</h3>
            <p>Wrfcoin implements confidential transactions that shield the amounts being transferred while maintaining the ability to verify that inputs equal outputs. This technology uses Pedersen commitments and range proofs to ensure transaction validity without revealing specific values, protecting users' financial privacy.</p>
        </div>
        
        <div class="feature">
            <h3>Private Smart Contracts</h3>
            <p>Our privacy-preserving smart contract system allows for confidential computation on encrypted data. By leveraging secure multi-party computation and homomorphic encryption techniques, Wrfcoin enables complex contract operations without exposing sensitive information to network participants.</p>
        </div>
        
        <div class="feature">
            <h3>Decentralized Mixing Service</h3>
            <p>The built-in decentralized mixing protocol allows users to break the transaction graph by pooling funds with other users and redistributing them according to specified outputs. This implementation uses zero-knowledge proofs to ensure mixing occurs correctly without revealing the connection between inputs and outputs.</p>
        </div>
    </div>
    
    <div class="section">
        <h2>Network Architecture</h2>
        
        <div class="feature">
            <h3>Specialized Node Architecture</h3>
            <p>Wrfcoin network consists of specialized nodes (validators, miners, relays, full, light, archive) that perform distinct functions within our ecosystem, with dynamic role assignment based on network needs. This tiered architecture enables different levels of participation, from lightweight mobile clients to high-performance validation servers, while maintaining consistent security guarantees across all node types through cryptographic proofs.</p>
        </div>

        <div class="feature">
            <h3>Geographic Optimization</h3>
            <p>Our node network is strategically distributed across geographic regions to enhance resilience, reduce latency, and ensure optimal data localization for computational NFTs focused on regional analytics. The network implements intelligent routing that minimizes cross-continental hops for transaction propagation and includes redundant storage of critical blockchain data with erasure coding to survive regional outages.</p>
        </div>
        
        <div class="feature">
            <h3>Peer Discovery and Reputation System</h3>
            <p>Wrfcoin implements an advanced peer discovery protocol using a combination of Kademlia-based DHT, DNS seeders, and optional centralized bootstrap nodes. Our reputation and scoring system tracks peer reliability metrics including uptime, bandwidth contribution, and validation accuracy to incentivize positive network participation and identify malicious nodes for quick isolation.</p>
        </div>
        
        <div class="feature">
            <h3>Bandwidth-Efficient Synchronization</h3>
            <p>New nodes joining the network benefit from optimized synchronization protocols that use compact block relay, accumulator-based UTXO set verification, and checkpoint-based fast sync options. This allows nodes to become operational in minutes instead of days, with multiple verification paths to ensure security during the bootstrap process even in adversarial network conditions.</p>
        </div>
    </div>
    
    <div class="section">
        <h2>MEV Protection</h2>
        
        <div class="feature">
            <h3>MEV Protection System</h3>
            <p>Our MEV (Miner Extractable Value) protection system ensures fair transaction ordering and prevents front-running, sandwich attacks, and other value extraction tactics to promote a more equitable blockchain ecosystem. The system uses encrypted mempools with time-locked transaction reveals and commit-reveal schemes to prevent miners from optimizing block construction for their own profit at users' expense.</p>
        </div>

        <div class="feature">
            <h3>Transaction Batching Fairness</h3>
            <p>The transaction batching system processes transactions in the order received and randomizes placement within blocks to neutralize timing advantages that would otherwise benefit sophisticated traders at others' expense. It enforces first-in-first-out (FIFO) ordering with cryptographic timestamps and includes minimum-stay requirements in the mempool to prevent rapid cancellation and resubmission attacks.</p>
        </div>
        
        <div class="feature">
            <h3>Fair Fee Distribution</h3>
            <p>To prevent miners from artificially inflating fees through self-transactions, Wrfcoin implements a fee distribution mechanism that shares a portion of transaction fees with all recent validators, rather than allocating all fees to the block producer. This removes the incentive for block producers to manipulate fee markets while still rewarding their essential validation work.</p>
        </div>
        
        <div class="feature">
            <h3>Off-Chain Auction Prevention</h3>
            <p>Our network monitoring system detects off-chain block space auctions by analyzing transaction inclusion patterns and fee distributions. When manipulation is detected, the system automatically adjusts consensus parameters to disincentivize such behavior. This ensures that the transaction fee market remains transparent and accessible to all participants rather than being captured by privileged groups.</p>
        </div>
    </div>
    
    <div class="section">
        <h2>Computational NFTs</h2>
        
        <div class="feature">
            <h3>NFT Implementation</h3>
            <p>Computational NFTs are unique tokens containing verifiable data analysis models that can be executed and validated by the network. They serve as both a store of value and a utility for data intelligence. Each NFT contains secure metadata, execution environments specs, input data references, execution code, and cryptographic verification paths that enable trustless computation across the network without revealing proprietary algorithm details.</p>
        </div>

        <div class="feature">
            <h3>Multi-Pool Verification</h3>
            <p>Our three-part NFT verification system includes multi-pool validation, execution framework testing, and accuracy assessment against real-world outcomes, ensuring computational NFTs deliver reliable, valuable results. The verification process uses a combination of deterministic execution environments, statistical sampling of results, and multi-party computation with threshold signatures to achieve consensus on model validity while preventing collusion.</p>
        </div>
        
        <div class="feature">
            <h3>Computation Marketplace</h3>
            <p>Wrfcoin provides a decentralized marketplace for computational resources, allowing NFT creators to bid for execution time from network participants. This creates an efficient market for specialized computing (GPU clusters, tensor processing units, quantum simulators) that matches supply with demand through on-chain auctions, smart contracts for service-level agreements, and reputation-based provider scoring.</p>
        </div>
        
        <div class="feature">
            <h3>Data Intelligence Rewards</h3>
            <p>The blockchain offers enhanced incentives for computational NFTs that contribute to solving real-world challenges in fields like climate science, medical research, and humanitarian intelligence. A decentralized oracle network verifies real-world impacts of computational models, with token rewards proportional to verified utility, creating a sustainable economy around useful decentralized computation.</p>
        </div>
    </div>
    
    <div class="section">
        <h2>Cross-Chain Features</h2>
        
        <div class="feature">
            <h3>Cross-Chain Interoperability</h3>
            <p>The cross-chain bridge allows Wrfcoin to be used on multiple blockchains, with a secure token locking mechanism on the home chain and minting of wrapped tokens on target chains, all protected by multi-signature validation. The bridge implements hash time-locked contracts (HTLCs) with threshold signature schemes that eliminate single points of failure while providing atomic swap guarantees across heterogeneous blockchain architectures.</p>
        </div>

        <div class="feature">
            <h3>Cross-Chain Messaging</h3>
            <p>Our cross-chain messaging protocol enables secure communication between Wrfcoin and other blockchains, facilitating complex multi-chain smart contract interactions and expanding the utility of Wrfcoin assets. The protocol uses a commitment-and-verification scheme with light client proofs to ensure messages cannot be forged or tampered with during cross-chain transmission, enabling trustless interoperability with Ethereum, Solana, Polygon, and other major networks.</p>
        </div>
        
        <div class="feature">
            <h3>Wrapped Asset Security</h3>
            <p>Wrapped versions of Wrfcoin on other chains include security mechanisms beyond standard multi-signature approaches. These include fault-tolerant validation committees that rotate membership, transparent reserve auditing with zero-knowledge proofs, and automated circuit breakers that can pause operations if suspicious activity is detected, keeping assets secure even if individual validators are compromised.</p>
        </div>
        
        <div class="feature">
            <h3>Interchain Account Management</h3>
            <p>Our interchain account system allows users to control assets across multiple blockchains through a single primary account on Wrfcoin. The system uses derivation paths with hierarchical deterministic key generation to create consistent cross-chain identities while maintaining separate private keys for each chain, combining convenience with security best practices for cross-chain operations.</p>
        </div>
    </div>
    
    <div class="section">
        <h2>Layer 2 Scaling</h2>
        
        <div class="feature">
            <h3>zkRollups</h3>
            <p>Wrfcoin implements zkRollups as our primary Layer 2 scaling solution, batching hundreds of transactions into a single proof, dramatically increasing throughput while maintaining the security guarantees of the main chain. Our zkRollup implementation uses recursive SNARK composition to aggregate multiple transaction batches into a single succinct proof, achieving up to 10,000 TPS while requiring minimal on-chain data for verification.</p>
        </div>

        <div class="feature">
            <h3>Optimistic Rollups</h3>
            <p>As an alternative scaling approach, our optimistic rollup implementation assumes transactions are valid by default and only requires proofs in case of disputes, offering flexibility for different throughput requirements. The implementation uses a sophisticated fraud proof system with incremental verification steps and reasonable challenge periods, balancing security with capital efficiency for validators who stake on transaction validity.</p>
        </div>
        
        <div class="feature">
            <h3>State Channels</h3>
            <p>For use cases requiring instant finality, Wrfcoin supports state channels that enable off-chain transactions with on-chain settlement only when necessary. Our implementation includes multi-hop payment routing, allowing participants to transact without direct channels, and watchtower services that protect offline users from malicious channel closures, making state channels practical for everyday use.</p>
        </div>
        
        <div class="feature">
            <h3>Layer 2 Interoperability</h3>
            <p>Wrfcoin's Layer 2 solutions are designed for cross-L2 compatibility, allowing direct transfers between different scaling solutions without going through the more expensive base layer. This is achieved through standardized state verification interfaces, shared sequencer networks where appropriate, and bridge contracts with uniform security models, creating a seamless user experience across the entire Wrfcoin ecosystem.</p>
        </div>
    </div>
    
    <div class="section">
        <h2>Governance</h2>
        
        <div class="feature">
            <h3>Enhanced Governance</h3>
            <p>Wrfcoin's governance system enables stakeholders to participate in protocol decisions through proposal creation, voting, and implementation, with quadratic voting to mitigate centralization risks and ensure fair representation. The system includes delegation capabilities for passive token holders, prediction markets for proposal outcomes to surface consensus early, and formal verification of code changes to prevent governance attacks.</p>
        </div>
        
        <div class="feature">
            <h3>Progressive Decentralization</h3>
            <p>Our governance follows a progressive decentralization roadmap with clear milestones for increasing community control. This includes gradual reduction of founding team privileges, growth of the validator set through permissionless entry requirements, and expansion of governance participation through user-friendly interfaces and education initiatives to maximize stakeholder involvement in decision-making.</p>
        </div>
        
        <div class="feature">
            <h3>On-Chain Treasury</h3>
            <p>The protocol maintains an on-chain treasury funded by a small percentage of transaction fees and managed through transparent governance. Treasury funds support ecosystem development, security audits, bug bounties, and community initiatives. All expenditures are publicly recorded on-chain with automated reporting and require multi-step approval processes to prevent misappropriation.</p>
        </div>
        
        <div class="feature">
            <h3>Futarchy and Prediction Markets</h3>
            <p>Wrfcoin governance incorporates prediction markets that allow the community to bet on the outcomes of proposed changes, creating economic incentives for revealing private information about proposal impacts. This mechanism helps optimize decision-making by measuring expected effects rather than just majority preferences, leading to more evidence-based governance.</p>
        </div>
    </div>
    
    <div class="section">
        <h2>Network Health Monitoring</h2>
        
        <div class="feature">
            <h3>Real-time Network Dashboard</h3>
            <p>Wrfcoin provides a comprehensive network health dashboard that visualizes key metrics including transaction throughput, block times, fee markets, validator participation, and network latency. The dashboard uses intuitive visualizations with color-coded status indicators to make complex blockchain data accessible to non-technical users while providing detailed drill-down capabilities for experts.</p>
        </div>
        
        <div class="feature">
            <h3>Decentralization Metrics</h3>
            <p>Our monitoring system includes specialized metrics for quantifying decentralization across multiple dimensions. These include Nakamoto coefficients for validator distribution, geographic diversity scores, client software diversity tracking, and stake concentration measurements. These metrics help identify centralization risks before they become critical vulnerabilities.</p>
        </div>
        
        <div class="feature">
            <h3>Economic Health Indicators</h3>
            <p>The dashboard tracks key economic indicators including token velocity, staking participation rates, circulation metrics, and fee market efficiency. These measurements provide early warning of potential imbalances in the tokenomic model and help governance participants make data-driven decisions about protocol parameters to maintain long-term sustainability.</p>
        </div>
        
        <div class="feature">
            <h3>Security Anomaly Detection</h3>
            <p>Our network monitoring includes advanced anomaly detection systems that identify unusual patterns in transaction flows, block production, network message propagation, and validator behavior. These systems use machine learning algorithms trained on historical attack patterns to provide early warnings of potential security threats, enabling rapid community response to emerging vulnerabilities.</p>
        </div>
    </div>
</body>
</html>"#
    );
    
    html
}