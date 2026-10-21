use crate::node::validator::{
    ValidatorManager, ValidatorInfo, ValidatorState, ValidatorMode, ConsensusMechanism,
    ValidatorEvent, StakeAmount, RewardDistribution
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

// Mock validator manager for testing
struct MockValidatorManager {
    validators: Arc<Mutex<HashMap<String, ValidatorInfo>>>,
    next_id: u32,
}

impl MockValidatorManager {
    fn new() -> Self {
        MockValidatorManager {
            validators: Arc::new(Mutex::new(HashMap::new())),
            next_id: 1,
        }
    }
    
    fn add_validator(&self, mut validator: ValidatorInfo) -> Result<ValidatorInfo, String> {
        let mut validators = self.validators.lock().unwrap();
        
        // Generate an ID if not provided
        if validator.id.is_empty() {
            validator.id = format!("validator-{}", self.next_id);
        }
        
        // Check if validator already exists
        if validators.contains_key(&validator.id) {
            return Err(format!("Validator with ID {} already exists", validator.id));
        }
        
        // Add the validator
        let validator_copy = validator.clone();
        validators.insert(validator.id.clone(), validator);
        
        Ok(validator_copy)
    }
    
    fn get_validator(&self, id: &str) -> Result<ValidatorInfo, String> {
        let validators = self.validators.lock().unwrap();
        
        match validators.get(id) {
            Some(validator) => Ok(validator.clone()),
            None => Err(format!("Validator with ID {} not found", id)),
        }
    }
    
    fn update_validator(&self, id: &str, updated: ValidatorInfo) -> Result<ValidatorInfo, String> {
        let mut validators = self.validators.lock().unwrap();
        
        if !validators.contains_key(id) {
            return Err(format!("Validator with ID {} not found", id));
        }
        
        let validator_copy = updated.clone();
        validators.insert(id.to_string(), updated);
        
        Ok(validator_copy)
    }
    
    fn list_validators(&self) -> Result<Vec<ValidatorInfo>, String> {
        let validators = self.validators.lock().unwrap();
        
        Ok(validators.values().cloned().collect())
    }
    
    fn remove_validator(&self, id: &str) -> Result<(), String> {
        let mut validators = self.validators.lock().unwrap();
        
        if !validators.contains_key(id) {
            return Err(format!("Validator with ID {} not found", id));
        }
        
        validators.remove(id);
        
        Ok(())
    }
    
    fn update_stake(&self, id: &str, amount: StakeAmount) -> Result<ValidatorInfo, String> {
        let mut validators = self.validators.lock().unwrap();
        
        if !validators.contains_key(id) {
            return Err(format!("Validator with ID {} not found", id));
        }
        
        let mut validator = validators.get(id).unwrap().clone();
        validator.stake = amount;
        
        validators.insert(id.to_string(), validator.clone());
        
        Ok(validator)
    }
    
    fn update_state(&self, id: &str, state: ValidatorState) -> Result<ValidatorInfo, String> {
        let mut validators = self.validators.lock().unwrap();
        
        if !validators.contains_key(id) {
            return Err(format!("Validator with ID {} not found", id));
        }
        
        let mut validator = validators.get(id).unwrap().clone();
        validator.state = state;
        
        validators.insert(id.to_string(), validator.clone());
        
        Ok(validator)
    }
}

#[test]
fn test_validator_creation() {
    let manager = MockValidatorManager::new();
    
    let validator = ValidatorInfo {
        id: "".to_string(), // Auto-generate ID
        address: "wrfvaloper1abc".to_string(),
        name: "Test Validator".to_string(),
        description: "A test validator".to_string(),
        state: ValidatorState::Pending,
        mode: ValidatorMode::Full,
        consensus_mechanism: ConsensusMechanism::ProofOfStake,
        stake: StakeAmount {
            self_stake: 1000,
            delegated_stake: 2000,
            total_stake: 3000,
        },
        commission_rate: 0.05,
        max_commission_rate: 0.1,
        commission_change_rate: 0.01,
        min_self_delegation: 500,
        rewards_address: "wrfrewards1abc".to_string(),
        website: "https://example.com".to_string(),
        identity: "ABCDEF0123456789".to_string(),
        security_contact: "security@example.com".to_string(),
        details: "Additional details".to_string(),
        height_created: 100,
        last_active_height: 100,
        uptime: 100.0,
        slash_points: 0,
        zone: "us-west".to_string(),
        supported_features: vec!["weatherData".to_string(), "computation".to_string()],
        reward_distribution: RewardDistribution {
            validators: 0.7,
            community_pool: 0.2,
            treasury: 0.1,
        },
    };
    
    // Add the validator
    let result = manager.add_validator(validator);
    assert!(result.is_ok());
    
    let added_validator = result.unwrap();
    
    // Verify the validator exists
    let validators = manager.list_validators().unwrap();
    assert_eq!(validators.len(), 1);
    
    // Remove the validator
    let remove_result = manager.remove_validator(&added_validator.id);
    assert!(remove_result.is_ok());
    
    // Verify the validator is gone
    let validators = manager.list_validators().unwrap();
    assert_eq!(validators.len(), 0);
    
    // Try to get a removed validator
    let get_result = manager.get_validator(&added_validator.id);
    assert!(get_result.is_err());
}