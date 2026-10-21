use std::fmt;

use serde::{Deserialize, Serialize};

use crate::crypto::hash::hash_bytes;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum CrossChainNetwork {
    Ethereum,
    Polygon,
    Avalanche,
    Arbitrum,
    Optimism,
    Base,
    Solana,
    Sui,
    Aptos,
    Cosmos,
    Polkadot,
    Custom(String),
}

impl CrossChainNetwork {
    pub fn as_id(&self) -> &str {
        match self {
            Self::Ethereum => "ethereum",
            Self::Polygon => "polygon",
            Self::Avalanche => "avalanche",
            Self::Arbitrum => "arbitrum",
            Self::Optimism => "optimism",
            Self::Base => "base",
            Self::Solana => "solana",
            Self::Sui => "sui",
            Self::Aptos => "aptos",
            Self::Cosmos => "cosmos",
            Self::Polkadot => "polkadot",
            Self::Custom(value) => value.as_str(),
        }
    }
}

impl fmt::Display for CrossChainNetwork {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_id())
    }
}

impl From<&str> for CrossChainNetwork {
    fn from(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "ethereum" => Self::Ethereum,
            "polygon" => Self::Polygon,
            "avalanche" => Self::Avalanche,
            "arbitrum" => Self::Arbitrum,
            "optimism" => Self::Optimism,
            "base" => Self::Base,
            "solana" => Self::Solana,
            "sui" => Self::Sui,
            "aptos" => Self::Aptos,
            "cosmos" => Self::Cosmos,
            "polkadot" => Self::Polkadot,
            other => Self::Custom(other.to_string()),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum CrossChainDirection {
    Inbound,
    Outbound,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum BridgeMessageType {
    Lock,
    Unlock,
    Mint,
    Burn,
    Attestation,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum RelayMessageStatus {
    Pending,
    Processing,
    Processed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BridgeMessage {
    pub message_id: String,
    pub source_network: CrossChainNetwork,
    pub target_network: CrossChainNetwork,
    pub direction: CrossChainDirection,
    pub message_type: BridgeMessageType,
    pub sender: String,
    pub recipient: String,
    pub amount: u64,
    pub nonce: u64,
    pub timestamp: i64,
    pub tx_hash: Option<String>,
    pub payload: Option<serde_json::Value>,
}

impl BridgeMessage {
    pub fn new(
        source_network: CrossChainNetwork,
        target_network: CrossChainNetwork,
        direction: CrossChainDirection,
        message_type: BridgeMessageType,
        sender: String,
        recipient: String,
        amount: u64,
        nonce: u64,
        timestamp: i64,
    ) -> Self {
        let preimage = format!(
            "{}|{}|{:?}|{:?}|{}|{}|{}|{}|{}",
            source_network,
            target_network,
            direction,
            message_type,
            sender,
            recipient,
            amount,
            nonce,
            timestamp
        );
        let message_id = format!("ccmsg-{}", hash_bytes(preimage.as_bytes()));
        Self {
            message_id,
            source_network,
            target_network,
            direction,
            message_type,
            sender,
            recipient,
            amount,
            nonce,
            timestamp,
            tx_hash: None,
            payload: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_id_is_deterministic_for_same_payload() {
        let a = BridgeMessage::new(
            CrossChainNetwork::Ethereum,
            CrossChainNetwork::Base,
            CrossChainDirection::Outbound,
            BridgeMessageType::Lock,
            "0xabc".to_string(),
            "0xdef".to_string(),
            42,
            7,
            1_700_000_000,
        );
        let b = BridgeMessage::new(
            CrossChainNetwork::Ethereum,
            CrossChainNetwork::Base,
            CrossChainDirection::Outbound,
            BridgeMessageType::Lock,
            "0xabc".to_string(),
            "0xdef".to_string(),
            42,
            7,
            1_700_000_000,
        );
        assert_eq!(a.message_id, b.message_id);
    }
}
