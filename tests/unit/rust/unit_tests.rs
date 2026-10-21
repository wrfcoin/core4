//! Unit testing framework for Wrfcoin core components
//! 
//! This module provides comprehensive unit tests for blockchain functionality,
//! climate data processing, and cross-chain operations.

use crate::test_utils::*;
use crate::mock_objects::*;

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_block_creation() {
        let _config = init_test_framework();
        let block = MockBlock::default();
        
        assert!(!block.hash.is_empty());
        assert!(!block.previous_hash.is_empty());
        assert_eq!(block.consensus_type, "proof_of_work");
        assert_eq!(block.status, "confirmed");
        assert!(block.timestamp > 0);
    }
    
    #[test]
    fn test_transaction_validation() {
        let _config = init_test_framework();
        let tx = MockTransaction::default();
        
        assert_valid_hash(&tx.hash);
        assert_valid_address(&tx.from);
        assert_valid_address(&tx.to);
        assert!(tx.amount > 0);
        assert!(tx.timestamp > 0);
    }
    
    #[test]
    fn test_climate_data_validation() {
        let _config = init_test_framework();
        let climate_data = MockClimateData::default();
        
        assert_climate_data_valid!(climate_data);
        assert!(!climate_data.location.is_empty());
        assert!(climate_data.timestamp > 0);
    }
    
    #[test]
    fn test_cross_chain_bridge() {
        let _config = init_test_framework();
        let bridge = MockCrossChainBridge::new();
        
        assert_eq!(bridge.get_bridge_status(), "active");
        assert!(bridge.get_supported_networks().len() > 0);
        assert!(bridge.supports_route("ethereum", "polygon"));
        assert_eq!(bridge.get_bridge_type(), "mock_bridge");
    }
    
    #[test]
    fn test_cross_chain_message_sending() {
        let _config = init_test_framework();
        let bridge = MockCrossChainBridge::new();
        
        let message = MockCrossChainMessage {
            from_network: "ethereum".to_string(),
            to_network: "polygon".to_string(),
            payload: vec![1, 2, 3, 4],
            timestamp: chrono::Utc::now().timestamp() as u64,
        };
        
        let result = bridge.send_message(message);
        assert!(result.is_ok());
        
        let message_id = result.unwrap();
        assert!(!message_id.is_empty());
        assert_eq!(bridge.get_message_status(&message_id), "confirmed");
    }
    
    #[test]
    fn test_consensus_mechanism() {
        let _config = init_test_framework();
        let mut consensus = MockConsensus::new("proof_of_work");
        
        let block = consensus.propose_block();
        assert!(consensus.validate_block(&block));
        assert_eq!(consensus.current_round, 2);
    }
    
    #[test]
    fn test_database_operations() {
        let _config = init_test_framework();
        let db = MockDatabase::new();
        
        // Test basic operations
        db.set("test_key", "test_value");
        assert_eq!(db.get("test_key"), Some("test_value".to_string()));
        
        assert!(db.delete("test_key"));
        assert_eq!(db.get("test_key"), None);
    }
    
    #[test]
    fn test_network_operations() {
        let _config = init_test_framework();
        let network = MockNetwork::new();
        
        assert!(network.get_peer_count() > 0);
        
        network.broadcast("test_message");
        network.send_to_peer("peer1", "direct_message");
        
        let messages = network.messages.lock().unwrap();
        assert_eq!(messages.len(), 2);
    }
    
    #[test]
    fn test_block_with_transactions() {
        let _config = init_test_framework();
        let block = create_test_block_with_transactions(5);
        
        assert_eq!(block.transactions.len(), 5);
        for (i, tx) in block.transactions.iter().enumerate() {
            assert_eq!(tx.hash, format!("tx_hash_{}", i));
            assert_eq!(tx.amount, (i + 1) as u64 * 10);
        }
    }
    
    #[test]
    fn test_climate_data_series() {
        let _config = init_test_framework();
        let readings = create_test_climate_readings(10);
        
        assert_eq!(readings.len(), 10);
        for (i, reading) in readings.iter().enumerate() {
            assert_climate_data_valid!(*reading);
            assert_eq!(reading.location, format!("location_{}", i));
        }
    }
    
    #[test]
    fn test_performance_timing() {
        let _config = init_test_framework();
        
        test_with_timeout!(
            std::time::Duration::from_millis(100),
            {
                // Simulate some work
                std::thread::sleep(std::time::Duration::from_millis(50));
                "test_completed"
            }
        );
    }
}

/// Integration tests for component interactions
#[cfg(test)]
mod integration_tests {
    use super::*;
    
    #[test]
    fn test_blockchain_climate_integration() {
        let _config = init_test_framework();
        let mut block = MockBlock::default();
        let climate_data = MockClimateData::default();
        
        // Add climate data to block metadata
        block.data_aware_metadata.insert(
            "climate_temperature".to_string(),
            climate_data.temperature.to_string()
        );
        block.data_aware_metadata.insert(
            "climate_location".to_string(),
            climate_data.location.clone()
        );
        
        assert!(block.data_aware_metadata.contains_key("climate_temperature"));
        assert!(block.data_aware_metadata.contains_key("climate_location"));
    }
    
    #[test]
    fn test_cross_chain_consensus_integration() {
        let _config = init_test_framework();
        let bridge = MockCrossChainBridge::new();
        let mut consensus = MockConsensus::new("proof_of_stake");
        
        let mut block = consensus.propose_block();
        
        // Add cross-chain metadata
        block.cross_chain_metadata.insert(
            "bridge_status".to_string(),
            bridge.get_bridge_status()
        );
        block.cross_chain_metadata.insert(
            "supported_networks".to_string(),
            bridge.get_supported_networks().join(",")
        );
        
        assert!(consensus.validate_block(&block));
        assert!(block.cross_chain_metadata.contains_key("bridge_status"));
    }
}