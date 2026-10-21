//! Governance module tests
//!
//! This file contains unit tests for the governance functionality,
//! including proposal management, voting, and treasury operations.

#[cfg(test)]
mod tests {
    use crate::governance::{
        GovernanceManager, 
        models::{
            Proposal, 
            ProposalStatus, 
            ProposalType, 
            VoteType
        },
        treasury::{Treasury, TreasuryTransactionType}
    };
    use crate::blockchain::state::account::Account;
    use std::collections::HashMap;

    // Helper function to create a test account
    fn create_test_account(id: &str, balance: u64) -> Account {
        Account {
            id: id.to_string(),
            balance,
            nonce: 0,
            public_key: "test_public_key".to_string(),
            metadata: HashMap::new(),
        }
    }

    #[test]
    fn test_create_proposal() {
        // Setup
        let mut governance = GovernanceManager::new();
        let proposer = create_test_account("alice", 1000);
        
        // Create a proposal
        let proposal_id = governance.create_proposal(
            &proposer,
            ProposalType::ParameterChange,
            "Test Proposal".to_string(),
            "This is a test proposal".to_string(),
            "Testing the proposal system".to_string(),
            r#"{"param": "value"}"#.to_string(),
            86400, // 1 day
            None
        ).expect("Failed to create proposal");
        
        // Verify the proposal was created
        let proposal = governance.proposal_storage.get_proposal(&proposal_id)
            .expect("Proposal not found");
        
        assert_eq!(proposal.proposal_type, ProposalType::ParameterChange);
        assert_eq!(proposal.status, ProposalStatus::Draft);
        assert_eq!(proposal.metadata.title, "Test Proposal");
        assert_eq!(proposal.proposer, "alice");
    }
    
    #[test]
    fn test_proposal_lifecycle() {
        // Setup
        let mut governance = GovernanceManager::new();
        let proposer = create_test_account("alice", 1000);
        let voter1 = create_test_account("bob", 500);
        let voter2 = create_test_account("charlie", 300);
        
        // Create a proposal
        let proposal_id = governance.create_proposal(
            &proposer,
            ProposalType::CommunityInitiative,
            "Community Event".to_string(),
            "Let's organize a community event".to_string(),
            "Building community engagement".to_string(),
            r#"{"event_type": "hackathon"}"#.to_string(),
            86400, // 1 day
            Some(43200) // 12 hours execution delay
        ).expect("Failed to create proposal");
        
        // Activate the proposal
        governance.activate_proposal(&proposal_id).expect("Failed to activate proposal");
        
        // Verify proposal is active
        let proposal = governance.proposal_storage.get_proposal(&proposal_id)
            .expect("Proposal not found");
        assert_eq!(proposal.status, ProposalStatus::Active);
        
        // Cast votes
        governance.vote_on_proposal(&voter1, &proposal_id, VoteType::For)
            .expect("Failed to vote");
        governance.vote_on_proposal(&voter2, &proposal_id, VoteType::Against)
            .expect("Failed to vote");
        
        // Verify vote counts
        let proposal = governance.proposal_storage.get_proposal(&proposal_id)
            .expect("Proposal not found");
        assert_eq!(proposal.votes_for, 500); // voter1's balance
        assert_eq!(proposal.votes_against, 300); // voter2's balance
        
        // Manually set voting end time to past to simulate voting period ending
        if let Some(proposal) = governance.proposal_storage.get_proposal_mut(&proposal_id) {
            proposal.voting_end = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() - 1;
        }
        
        // Finalize the proposal
        let results = governance.finalize_proposal(&proposal_id).expect("Failed to finalize proposal");
        
        // Verify the proposal passed
        assert!(results.passed);
        
        // Verify proposal status
        let proposal = governance.proposal_storage.get_proposal(&proposal_id)
            .expect("Proposal not found");
        assert_eq!(proposal.status, ProposalStatus::Passed);
        
        // Set execution delay to the past
        if let Some(proposal) = governance.proposal_storage.get_proposal_mut(&proposal_id) {
            proposal.voting_end = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() - 43201; // past the execution delay
        }
        
        // Execute the proposal
        governance.execute_proposal(&proposal_id).expect("Failed to execute proposal");
        
        // Verify the proposal is executed
        let proposal = governance.proposal_storage.get_proposal(&proposal_id)
            .expect("Proposal not found");
        assert_eq!(proposal.status, ProposalStatus::Executed);
    }
    
    #[test]
    fn test_treasury_operations() {
        // Setup initial treasury
        let mut treasury = Treasury::new(
            vec!["alice".to_string(), "bob".to_string(), "charlie".to_string()],
            2 // Require 2 signatures
        );
        
        // Test deposit
        let deposit_id = treasury.deposit(
            1000,
            "source_account".to_string(),
            "Initial funding".to_string()
        ).expect("Failed to deposit");
        
        // Verify balance
        assert_eq!(treasury.balance, 1000);
        
        // Verify transaction record
        let transactions = treasury.get_transaction_history();
        assert_eq!(transactions.len(), 1);
        assert_eq!(transactions[0].transaction_type, TreasuryTransactionType::Deposit);
        assert_eq!(transactions[0].amount, 1000);
        
        // Test withdrawal with sufficient signatures
        let withdrawal_id = treasury.withdraw(
            500,
            "recipient".to_string(),
            Some("proposal_123".to_string()),
            "Project funding".to_string(),
            &["alice".to_string(), "bob".to_string()]
        ).expect("Failed to withdraw");
        
        // Verify updated balance
        assert_eq!(treasury.balance, 500);
        
        // Verify transaction record
        let transactions = treasury.get_transaction_history();
        assert_eq!(transactions.len(), 2);
        assert_eq!(transactions[1].transaction_type, TreasuryTransactionType::Withdrawal);
        assert_eq!(transactions[1].amount, 500);
        assert_eq!(transactions[1].receiver, Some("recipient".to_string()));
        
        // Test withdrawal with insufficient signatures
        let result = treasury.withdraw(
            200,
            "recipient".to_string(),
            None,
            "Unauthorized withdrawal".to_string(),
            &["alice".to_string()]
        );
        
        // Verify withdrawal failed
        assert!(result.is_err());
        
        // Verify balance unchanged
        assert_eq!(treasury.balance, 500);
    }
    
    #[test]
    fn test_budget_categories() {
        // Setup
        let mut treasury = Treasury::new(
            vec!["admin".to_string()],
            1
        );
        
        // Initial deposit
        treasury.deposit(10000, "source".to_string(), "Initial funding".to_string())
            .expect("Failed to deposit");
        
        // Create budget categories
        let development_id = treasury.add_budget_category(
            "Development".to_string(),
            "Funding for development activities".to_string(),
            5000,
            Some(7000)
        ).expect("Failed to create category");
        
        let marketing_id = treasury.add_budget_category(
            "Marketing".to_string(),
            "Funding for marketing activities".to_string(),
            3000,
            None
        ).expect("Failed to create category");
        
        // Verify categories
        let categories = treasury.get_budget_categories();
        assert_eq!(categories.len(), 2);
        
        // Verify treasury balance after allocations
        assert_eq!(treasury.balance, 2000); // 10000 - 5000 - 3000
        
        // Update allocation - increase
        treasury.update_category_allocation(&development_id, 6000)
            .expect("Failed to update allocation");
        
        // Verify updated treasury balance
        assert_eq!(treasury.balance, 1000); // 2000 - 1000 (increase)
        
        // Update allocation - decrease
        treasury.update_category_allocation(&marketing_id, 2000)
            .expect("Failed to update allocation");
        
        // Verify updated treasury balance
        assert_eq!(treasury.balance, 2000); // 1000 + 1000 (decrease)
        
        // Test allocation cap enforcement
        let result = treasury.update_category_allocation(&development_id, 8000);
        assert!(result.is_err()); // Should fail due to allocation cap of 7000
        
        // Verify balance unchanged
        assert_eq!(treasury.balance, 2000);
    }
    
    #[test]
    fn test_treasury_signatories() {
        // Setup
        let mut treasury = Treasury::new(
            vec!["alice".to_string(), "bob".to_string()],
            2
        );
        
        // Add a new signatory
        treasury.add_signatory("charlie".to_string())
            .expect("Failed to add signatory");
        
        // Verify signatory list
        assert_eq!(treasury.signatories.len(), 3);
        
        // Initial deposit
        treasury.deposit(1000, "source".to_string(), "Initial funding".to_string())
            .expect("Failed to deposit");
        
        // Test withdrawal with new signatory
        let result = treasury.withdraw(
            500,
            "recipient".to_string(),
            None,
            "Test withdrawal".to_string(),
            &["alice".to_string(), "charlie".to_string()]
        );
        
        // Verify withdrawal succeeded with new signatory
        assert!(result.is_ok());
        
        // Remove a signatory
        treasury.remove_signatory("bob")
            .expect("Failed to remove signatory");
        
        // Verify signatory list update
        assert_eq!(treasury.signatories.len(), 2);
        assert!(!treasury.signatories.contains(&"bob".to_string()));
        
        // Update signature threshold
        treasury.update_signature_threshold(1)
            .expect("Failed to update threshold");
        
        // Verify new threshold
        assert_eq!(treasury.required_signatures, 1);
        
        // Test withdrawal with reduced threshold
        let result = treasury.withdraw(
            200,
            "recipient".to_string(),
            None,
            "Test threshold".to_string(),
            &["alice".to_string()]
        );
        
        // Verify withdrawal succeeded with reduced threshold
        assert!(result.is_ok());
    }
    
    #[test]
    fn test_proposal_voting_restrictions() {
        // Setup
        let mut governance = GovernanceManager::new();
        let proposer = create_test_account("alice", 1000);
        let voter = create_test_account("bob", 500);
        
        // Create a proposal
        let proposal_id = governance.create_proposal(
            &proposer,
            ProposalType::ParameterChange,
            "Test Proposal".to_string(),
            "This is a test proposal".to_string(),
            "Testing the proposal system".to_string(),
            r#"{"param": "value"}"#.to_string(),
            86400, // 1 day
            None
        ).expect("Failed to create proposal");
        
        // Try to vote on draft proposal (should fail)
        let result = governance.vote_on_proposal(&voter, &proposal_id, VoteType::For);
        assert!(result.is_err());
        
        // Activate the proposal
        governance.activate_proposal(&proposal_id).expect("Failed to activate proposal");
        
        // Vote on active proposal (should succeed)
        let result = governance.vote_on_proposal(&voter, &proposal_id, VoteType::For);
        assert!(result.is_ok());
        
        // Try to vote again (should fail due to double-voting restriction)
        let result = governance.vote_on_proposal(&voter, &proposal_id, VoteType::Against);
        assert!(result.is_err());
    }
}