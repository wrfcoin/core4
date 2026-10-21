#[cfg(test)]
mod connectivity_tests {
    /// This test verifies that we can instantiate and interact with core components
    /// across different modules after the SDK to core migration.
    #[test]
    fn test_core_blockchain_module_connectivity() {
        // Import core components
        use wrfcoin::core::error::WrfError;
        use wrfcoin::blockchain::state::account::Account;
        use wrfcoin::blockchain::state::nft::StandardNFT;
        use wrfcoin::blockchain::block::Block;
        
        // Create a test account to verify blockchain module functionality
        let account = Account::new(
            "test_account".to_string(),
            [0u8; 32],
            100,
            None,
        );
        
        // Verify account properties
        assert_eq!(account.balance, 100);
        assert_eq!(account.id, "test_account");
        
        // Create an NFT to verify NFT module connectivity
        let nft = StandardNFT::new(
            "test_nft_1".to_string(),
            "Test NFT".to_string(),
            "A test NFT description".to_string(),
            "test_account".to_string(),
            "image_hash".to_string(),
            None,
        );
        
        // Verify NFT properties
        assert_eq!(nft.id, "test_nft_1");
        assert_eq!(nft.owner, "test_account");
    }
    
    /// This test verifies that the consensus module correctly integrates with
    /// the blockchain module after the migration.
    #[test]
    fn test_consensus_blockchain_integration() {
        // Import components from different modules
        use wrfcoin::blockchain::block::Block;
        use wrfcoin::consensus::pos::ProofOfStake;
        
        // Create a new consensus instance
        let pos = ProofOfStake::new(vec!["validator1".to_string()], 1000);
        
        // Verify PoS validator count
        assert_eq!(pos.validators.len(), 1);
    }
    
    /// This test verifies that the cross-chain module correctly integrates with
    /// the blockchain module after the migration.
    #[test]
    fn test_cross_chain_blockchain_integration() {
        // Import components from different modules
        use wrfcoin::blockchain::block::Block;
        use wrfcoin::cross_chain::bridge::AxelarBridge;
        
        // Create a cross-chain bridge instance
        let bridge = AxelarBridge::new("testnet".to_string());
        
        // Verify bridge network
        assert_eq!(bridge.network, "testnet");
    }
}