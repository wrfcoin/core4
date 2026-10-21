use serde::{Serialize, Deserialize};
use std::fmt;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessageType {
    GetBlocks,
    Blocks,
    GetTransactions,
    Transactions,
    NewBlock,
    NewTransaction,
    GetPeers,
    Peers,
    Ping,
    Pong,
    Error,
}

impl fmt::Display for MessageType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MessageType::GetBlocks => write!(f, "GetBlocks"),
            MessageType::Blocks => write!(f, "Blocks"),
            MessageType::GetTransactions => write!(f, "GetTransactions"),
            MessageType::Transactions => write!(f, "Transactions"),
            MessageType::NewBlock => write!(f, "NewBlock"),
            MessageType::NewTransaction => write!(f, "NewTransaction"),
            MessageType::GetPeers => write!(f, "GetPeers"),
            MessageType::Peers => write!(f, "Peers"),
            MessageType::Ping => write!(f, "Ping"),
            MessageType::Pong => write!(f, "Pong"),
            MessageType::Error => write!(f, "Error"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub message_type: MessageType,
    pub payload: Vec<u8>,
    pub timestamp: u64,
    pub sender: String,
}

impl Message {
    pub fn new(message_type: MessageType, payload: Vec<u8>, sender: String) -> Self {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        
        Message {
            message_type,
            payload,
            timestamp,
            sender,
        }
    }
    
    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_default()
    }
    
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, Box<dyn std::error::Error>> {
        let message: Message = serde_json::from_slice(bytes)?;
        Ok(message)
    }
}

pub struct Protocol {
    pub version: String,
    pub min_compatible_version: String,
}

impl Protocol {
    pub fn new() -> Self {
        Protocol {
            version: "1.0.0".to_string(),
            min_compatible_version: "1.0.0".to_string(),
        }
    }
    
    pub fn is_compatible(&self, other_version: &str) -> bool {
        // Simple version check - in a real implementation this would be more sophisticated
        // to handle semantic versioning properly
        other_version >= &self.min_compatible_version
    }
    
    pub fn get_version(&self) -> String {
        self.version.clone()
    }
}

pub fn create_get_blocks_message(start_height: u64, end_height: u64, sender: String) -> Message {
    let payload = serde_json::to_vec(&(start_height, end_height)).unwrap_or_default();
    Message::new(MessageType::GetBlocks, payload, sender)
}

pub fn create_blocks_message(blocks_json: Vec<u8>, sender: String) -> Message {
    Message::new(MessageType::Blocks, blocks_json, sender)
}

pub fn create_new_block_message(block_json: Vec<u8>, sender: String) -> Message {
    Message::new(MessageType::NewBlock, block_json, sender)
}

pub fn create_new_transaction_message(tx_json: Vec<u8>, sender: String) -> Message {
    Message::new(MessageType::NewTransaction, tx_json, sender)
}

pub fn create_get_peers_message(sender: String) -> Message {
    Message::new(MessageType::GetPeers, vec![], sender)
}

pub fn create_peers_message(peers_json: Vec<u8>, sender: String) -> Message {
    Message::new(MessageType::Peers, peers_json, sender)
}

pub fn create_ping_message(sender: String) -> Message {
    Message::new(MessageType::Ping, vec![], sender)
}

pub fn create_pong_message(sender: String) -> Message {
    Message::new(MessageType::Pong, vec![], sender)
}

pub fn create_error_message(error_msg: &str, sender: String) -> Message {
    Message::new(MessageType::Error, error_msg.as_bytes().to_vec(), sender)
}
