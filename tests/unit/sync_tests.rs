use crate::node::sync::{SyncManager, SyncOptions, SyncMode, SyncStrategy};
use crate::node::sync::{BlockValidator, TransactionValidator, SyncEvent, SyncStatus};
use std::sync::{Arc, Mutex};

// Mock implementations for testing
struct MockBlockValidator;
impl BlockValidator for MockBlockValidator {
    fn validate_block(&self, block_data: &[u8]) -> Result<bool, String> {
        // For testing, just validate based on block data length
        // In a real implementation, this would check signatures, hash, etc.
        Ok(block_data.len() > 10)
    }
}

struct MockTransactionValidator;
impl TransactionValidator for MockTransactionValidator {
    fn validate_transaction(&self, tx_data: &[u8]) -> Result<bool, String> {
        // For testing, just validate based on transaction data length
        // In a real implementation, this would check signatures, inputs, outputs, etc.
        Ok(tx_data.len() > 5)
    }
}

// Struct to capture sync events for testing
struct SyncEventCollector {
    events: Arc<Mutex<Vec<SyncEvent>>>,
}

impl SyncEventCollector {
    fn new() -> Self {
        SyncEventCollector {
            events: Arc::new(Mutex::new(Vec::new())),
        }
    }
    
    fn add_event(&self, event: SyncEvent) {
        let mut events = self.events.lock().unwrap();
        events.push(event);
    }
    
    fn get_events(&self) -> Vec<SyncEvent> {
        let events = self.events.lock().unwrap();
        events.clone()
    }
}

#[test]
fn test_sync_manager_initialization() {
    let options = SyncOptions {
        mode: SyncMode::Full,
        strategy: SyncStrategy::Parallel,
        max_parallel_blocks: 10,
        start_height: 0,
        target_height: 1000,
        skip_validation: false,
    };
    
    let sync_manager = SyncManager::new(options);
    
    assert_eq!(sync_manager.get_status(), SyncStatus::NotStarted);
    assert_eq!(sync_manager.get_current_height(), 0);
    assert_eq!(sync_manager.get_target_height(), 1000);
}

#[test]
fn test_sync_manager_block_validation() {
    let options = SyncOptions {
        mode: SyncMode::Full,
        strategy: SyncStrategy::Parallel,
        max_parallel_blocks: 10,
        start_height: 0,
        target_height: 1000,
        skip_validation: false,
    };
    
    let mut sync_manager = SyncManager::new(options);
    
    // Set the validators
    sync_manager.set_block_validator(MockBlockValidator);
    sync_manager.set_transaction_validator(MockTransactionValidator);
    
    // Test valid block
    let valid_block = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];
    assert!(sync_manager.validate_block(&valid_block).unwrap());
    
    // Test invalid block
    let invalid_block = vec![1, 2, 3, 4];
    assert!(!sync_manager.validate_block(&invalid_block).unwrap());
    
    // Test valid transaction
    let valid_tx = vec![1, 2, 3, 4, 5, 6];
    assert!(sync_manager.validate_transaction(&valid_tx).unwrap());
    
    // Test invalid transaction
    let invalid_tx = vec![1, 2, 3];
    assert!(!sync_manager.validate_transaction(&invalid_tx).unwrap());
}

#[test]
fn test_sync_manager_with_events() {
    let options = SyncOptions {
        mode: SyncMode::Headers,
        strategy: SyncStrategy::Sequential,
        max_parallel_blocks: 1,
        start_height: 0,
        target_height: 100,
        skip_validation: true,
    };
    
    let mut sync_manager = SyncManager::new(options);
    
    // Create event collector
    let event_collector = SyncEventCollector::new();
    let event_collector_clone = event_collector.clone();
    
    // Register event handler
    sync_manager.on_event(move |event| {
        event_collector_clone.add_event(event.clone());
    });
    
    // Start the sync
    sync_manager.start().unwrap();
    
    // Force a status change
    sync_manager.set_status(SyncStatus::InProgress);
    
    // Update progress
    for height in 1..=10 {
        sync_manager.set_current_height(height);
    }
    
    // Complete the sync
    sync_manager.set_status(SyncStatus::Completed);
    
    // Check that events were recorded
    let events = event_collector.get_events();
    
    // We should have at least 3 events:
    // 1. Started event
    // 2. Progress events (at least one)
    // 3. Completed event
    assert!(events.len() >= 3);
    
    // Check the first event is Started
    match &events[0] {
        SyncEvent::Started { mode, strategy, .. } => {
            assert_eq!(*mode, SyncMode::Headers);
            assert_eq!(*strategy, SyncStrategy::Sequential);
        },
        _ => panic!("First event should be Started"),
    }
    
    // Check the last event is Completed
    match &events[events.len() - 1] {
        SyncEvent::Completed { final_height, .. } => {
            assert_eq!(*final_height, 10);
        },
        _ => panic!("Last event should be Completed"),
    }
}