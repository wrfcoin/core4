/**
 * HSM Integration Tests
 * Comprehensive testing of Hardware Security Module functionality
 */

use crate::wallet::hsm::{
    HsmManager, HsmConfig, HsmProvider, HsmKeyType, SoftHsm, HsmInterface, HsmResult
};
use crate::wallet::hsm_operations::{
    HsmWalletOperations, HsmWalletConfig, DerivationPath, HsmPermission
};
use std::sync::{Arc, Mutex};

#[test]
fn test_hsm_interface_lifecycle() {
    let mut hsm = SoftHsm::new();
    
    // Test configuration
    let config = HsmConfig {
        provider: HsmProvider::SoftHsm,
        connection_string: "test-connection".to_string(),
        slot_id: Some(0),
        pin: Some("1234".to_string()),
        library_path: None,
        timeout_ms: 5000,
    };
    
    // Test initialization
    assert!(matches!(hsm.initialize(&config), HsmResult::Success(())));
    
    // Test authentication
    assert!(matches!(hsm.authenticate("1234"), HsmResult::Success(())));
    
    // Test status check
    let status = hsm.get_status();
    assert!(matches!(status, HsmResult::Success(_)));
    
    if let HsmResult::Success(status_info) = status {
        assert!(status_info.connected);
        assert!(status_info.authenticated);
        assert_eq!(status_info.firmware_version, "SoftHSM-Dev-1.0");
    }
}

#[test]
fn test_hsm_key_operations() {
    let mut hsm = SoftHsm::new();
    let config = HsmConfig {
        provider: HsmProvider::SoftHsm,
        connection_string: "test".to_string(),
        slot_id: Some(0),
        pin: Some("1234".to_string()),
        library_path: None,
        timeout_ms: 5000,
    };
    
    // Initialize and authenticate
    hsm.initialize(&config).unwrap();
    hsm.authenticate("1234").unwrap();
    
    // Test key generation
    let key_result = hsm.generate_key_pair(
        HsmKeyType::EcdsaSecp256k1,
        256,
        "test-wallet-key",
        false,
    );
    
    assert!(matches!(key_result, HsmResult::Success(_)));
    
    if let HsmResult::Success(key_info) = key_result {
        assert_eq!(key_info.label, "test-wallet-key");
        assert_eq!(key_info.key_size, 256);
        assert!(!key_info.extractable);
        assert!(key_info.can_sign);
        
        // Test signing with the generated key
        let test_data = b"test transaction data";
        let sign_result = hsm.sign(&key_info.key_id, test_data, "ECDSA-SHA256");
        assert!(matches!(sign_result, HsmResult::Success(_)));
        
        // Test signature verification
        if let HsmResult::Success(signature) = sign_result {
            let verify_result = hsm.verify(&key_info.key_id, test_data, &signature, "ECDSA-SHA256");
            assert!(matches!(verify_result, HsmResult::Success(true)));
        }
        
        // Test key listing
        let list_result = hsm.list_keys();
        assert!(matches!(list_result, HsmResult::Success(_)));
        
        if let HsmResult::Success(keys) = list_result {
            assert_eq!(keys.len(), 1);
            assert_eq!(keys[0].key_id, key_info.key_id);
        }
        
        // Test key info retrieval
        let info_result = hsm.get_key_info(&key_info.key_id);
        assert!(matches!(info_result, HsmResult::Success(_)));
        
        // Test key deletion
        let delete_result = hsm.delete_key(&key_info.key_id);
        assert!(matches!(delete_result, HsmResult::Success(())));
        
        // Verify key is deleted
        let list_after_delete = hsm.list_keys();
        if let HsmResult::Success(keys) = list_after_delete {
            assert_eq!(keys.len(), 0);
        }
    }
}

#[test]
fn test_hsm_manager_functionality() {
    let mut manager = HsmManager::new();
    
    // Add HSM instance
    let hsm = Box::new(SoftHsm::new());
    assert!(matches!(manager.add_hsm("test-hsm".to_string(), hsm), HsmResult::Success(())));
    
    // Test HSM initialization
    let config = HsmConfig {
        provider: HsmProvider::SoftHsm,
        connection_string: "test".to_string(),
        slot_id: Some(0),
        pin: Some("1234".to_string()),
        library_path: None,
        timeout_ms: 5000,
    };
    
    assert!(matches!(manager.initialize_hsm("test-hsm", &config), HsmResult::Success(())));
    
    // Test wallet key generation
    let key_result = manager.generate_wallet_key("test-wallet-master");
    assert!(matches!(key_result, HsmResult::Success(_)));
    
    // Test health status
    let status_map = manager.get_health_status();
    assert!(status_map.contains_key("test-hsm"));
    assert!(status_map["test-hsm"].connected);
    
    // Test audit log
    let audit_log = manager.export_audit_log();
    assert!(audit_log.len() >= 3); // HSM_ADDED, HSM_INITIALIZED, WALLET_KEY_GENERATED
    
    // Verify audit log entries
    assert!(audit_log.iter().any(|entry| entry.operation == "HSM_ADDED"));
    assert!(audit_log.iter().any(|entry| entry.operation == "HSM_INITIALIZED"));
    assert!(audit_log.iter().any(|entry| entry.operation == "WALLET_KEY_GENERATED"));
}

#[test]
fn test_hsm_wallet_operations_complete_flow() {
    // Setup HSM manager
    let manager = Arc::new(Mutex::new(HsmManager::new()));
    let hsm = Box::new(SoftHsm::new());
    
    {
        let mut mgr = manager.lock().unwrap();
        mgr.add_hsm("default".to_string(), hsm).unwrap();
        
        let config = HsmConfig {
            provider: HsmProvider::SoftHsm,
            connection_string: "test".to_string(),
            slot_id: Some(0),
            pin: Some("1234".to_string()),
            library_path: None,
            timeout_ms: 5000,
        };
        
        mgr.initialize_hsm("default", &config).unwrap();
    }
    
    let mut wallet_ops = HsmWalletOperations::new(manager.clone());
    
    // Test wallet initialization
    let wallet_config = HsmWalletConfig {
        master_key_label: "integration-test-master".to_string(),
        auto_authenticate: true,
        key_backup_enabled: true,
        audit_logging: true,
        session_timeout_minutes: 30,
    };
    
    let master_key_id = wallet_ops.initialize_wallet(&wallet_config).unwrap();
    assert!(!master_key_id.is_empty());
    
    // Test multiple key derivations
    let paths = vec![
        DerivationPath::wrfcoin_standard(0, 0, 0),
        DerivationPath::wrfcoin_standard(0, 0, 1),
        DerivationPath::wrfcoin_standard(0, 1, 0),
        DerivationPath::wrfcoin_standard(1, 0, 0),
    ];
    
    let mut derived_keys = Vec::new();
    for path in &paths {
        let key_id = wallet_ops.derive_key(path).unwrap();
        derived_keys.push(key_id);
        assert!(!derived_keys.last().unwrap().is_empty());
    }
    
    // Test transaction signing with each derived key
    let transaction_hash = b"test transaction hash for signing";
    for (i, path) in paths.iter().enumerate() {
        let signature = wallet_ops.sign_transaction_hsm(path, transaction_hash).unwrap();
        assert!(!signature.is_empty());
        assert!(signature.len() > 10); // Basic signature length check
    }
    
    // Test backup and restore functionality
    let backup_password = "secure_backup_password_123!";
    let backup_info = wallet_ops.create_backup(backup_password).unwrap();
    
    assert!(!backup_info.backup_id.is_empty());
    assert!(backup_info.key_ids.len() >= 5); // Master + 4 derived keys
    assert!(!backup_info.encrypted_backup.is_empty());
    assert!(!backup_info.checksum.is_empty());
    
    // Test backup restoration
    let restore_result = wallet_ops.restore_from_backup(&backup_info, backup_password);
    assert!(restore_result.is_ok());
    
    // Test HSM status retrieval
    let status_json = wallet_ops.get_hsm_status().unwrap();
    assert!(status_json["derived_keys_count"].as_u64().unwrap() >= 4);
    assert!(status_json["audit_entries_count"].as_u64().unwrap() > 0);
}

#[test]
fn test_derivation_path_functionality() {
    // Test standard Wrfcoin derivation paths
    let path1 = DerivationPath::wrfcoin_standard(0, 0, 0);
    assert_eq!(path1.to_string(), "m/44'/1337'/0'/0/0");
    
    let path2 = DerivationPath::wrfcoin_standard(2, 1, 100);
    assert_eq!(path2.to_string(), "m/44'/1337'/2'/1/100");
    
    // Test custom derivation path
    let custom_path = DerivationPath {
        purpose: 84,     // BIP-84 (Native SegWit)
        coin_type: 1337, // Wrfcoin
        account: 0,
        change: 0,
        address_index: 5,
    };
    assert_eq!(custom_path.to_string(), "m/84'/1337'/0'/0/5");
}

#[test]
fn test_access_control_system() {
    let manager = Arc::new(Mutex::new(HsmManager::new()));
    let wallet_ops = HsmWalletOperations::new(manager);
    
    // Test different user permission levels
    let test_cases = vec![
        ("user1", HsmPermission::Sign, None, true),
        ("user1", HsmPermission::Encrypt, None, true),
        ("user1", HsmPermission::GenerateKey, None, false),
        ("admin_user", HsmPermission::GenerateKey, None, true),
        ("admin_user", HsmPermission::ImportKey, None, true),
        ("admin_user", HsmPermission::DeleteKey, None, false),
        ("admin_root", HsmPermission::DeleteKey, None, true),
        ("admin_root", HsmPermission::ManageAccess, None, true),
        ("regular_user", HsmPermission::ManageAccess, None, false),
    ];
    
    for (user_id, permission, key_id, expected) in test_cases {
        let result = wallet_ops.check_access_permission(user_id, &permission, key_id);
        assert_eq!(result.unwrap(), expected, 
            "Failed for user: {}, permission: {:?}", user_id, permission);
    }
}

#[test]
fn test_key_rotation_functionality() {
    let manager = Arc::new(Mutex::new(HsmManager::new()));
    let hsm = Box::new(SoftHsm::new());
    
    {
        let mut mgr = manager.lock().unwrap();
        mgr.add_hsm("rotation-test".to_string(), hsm).unwrap();
        
        let config = HsmConfig {
            provider: HsmProvider::SoftHsm,
            connection_string: "test".to_string(),
            slot_id: Some(0),
            pin: Some("1234".to_string()),
            library_path: None,
            timeout_ms: 5000,
        };
        
        mgr.initialize_hsm("rotation-test", &config).unwrap();
    }
    
    let mut wallet_ops = HsmWalletOperations::new(manager);
    
    // Initialize wallet
    let wallet_config = HsmWalletConfig {
        master_key_label: "rotation-test-master".to_string(),
        auto_authenticate: true,
        key_backup_enabled: true,
        audit_logging: true,
        session_timeout_minutes: 30,
    };
    
    let original_master = wallet_ops.initialize_wallet(&wallet_config).unwrap();
    
    // Derive some keys
    let paths = vec![
        DerivationPath::wrfcoin_standard(0, 0, 0),
        DerivationPath::wrfcoin_standard(0, 0, 1),
    ];
    
    for path in &paths {
        wallet_ops.derive_key(path).unwrap();
    }
    
    // Test master key rotation
    let rotated_keys = wallet_ops.rotate_keys("master_only").unwrap();
    assert_eq!(rotated_keys.len(), 1);
    assert_ne!(rotated_keys[0], original_master);
    
    // Test derived key rotation
    let rotated_derived = wallet_ops.rotate_keys("all_derived").unwrap();
    assert_eq!(rotated_derived.len(), 2); // 2 derived keys were rotated
}

#[test]
fn test_backup_encryption_integrity() {
    let manager = Arc::new(Mutex::new(HsmManager::new()));
    let hsm = Box::new(SoftHsm::new());
    
    {
        let mut mgr = manager.lock().unwrap();
        mgr.add_hsm("backup-test".to_string(), hsm).unwrap();
        
        let config = HsmConfig {
            provider: HsmProvider::SoftHsm,
            connection_string: "test".to_string(),
            slot_id: Some(0),
            pin: Some("1234".to_string()),
            library_path: None,
            timeout_ms: 5000,
        };
        
        mgr.initialize_hsm("backup-test", &config).unwrap();
    }
    
    let mut wallet_ops = HsmWalletOperations::new(manager);
    
    // Initialize wallet with keys
    let wallet_config = HsmWalletConfig {
        master_key_label: "backup-test-master".to_string(),
        auto_authenticate: true,
        key_backup_enabled: true,
        audit_logging: true,
        session_timeout_minutes: 30,
    };
    
    wallet_ops.initialize_wallet(&wallet_config).unwrap();
    
    // Create multiple derived keys
    for i in 0..3 {
        let path = DerivationPath::wrfcoin_standard(0, 0, i);
        wallet_ops.derive_key(&path).unwrap();
    }
    
    // Test backup with correct password
    let correct_password = "correct_backup_password_123!";
    let backup = wallet_ops.create_backup(correct_password).unwrap();
    
    // Test restore with correct password
    assert!(wallet_ops.restore_from_backup(&backup, correct_password).is_ok());
    
    // Test restore with incorrect password (should fail)
    let wrong_password = "wrong_password";
    assert!(wallet_ops.restore_from_backup(&backup, wrong_password).is_err());
    
    // Test backup integrity by modifying checksum
    let mut corrupted_backup = backup.clone();
    corrupted_backup.checksum = "invalid_checksum".to_string();
    assert!(wallet_ops.restore_from_backup(&corrupted_backup, correct_password).is_err());
    
    // Test backup integrity by modifying encrypted data
    let mut corrupted_data_backup = backup.clone();
    corrupted_data_backup.encrypted_backup[0] ^= 0xFF; // Flip some bits
    assert!(wallet_ops.restore_from_backup(&corrupted_data_backup, correct_password).is_err());
}

#[test]
fn test_hsm_error_handling() {
    let mut hsm = SoftHsm::new();
    
    // Test operations without initialization
    let result = hsm.authenticate("1234");
    assert!(matches!(result, HsmResult::Error(_)));
    
    // Test operations without authentication
    let config = HsmConfig {
        provider: HsmProvider::SoftHsm,
        connection_string: "test".to_string(),
        slot_id: Some(0),
        pin: Some("1234".to_string()),
        library_path: None,
        timeout_ms: 5000,
    };
    
    hsm.initialize(&config).unwrap();
    
    // Try to generate key without authentication
    let key_result = hsm.generate_key_pair(
        HsmKeyType::EcdsaSecp256k1,
        256,
        "test-key",
        false,
    );
    assert!(matches!(key_result, HsmResult::Error(_)));
    
    // Authenticate and test key operations
    hsm.authenticate("1234").unwrap();
    
    // Test operations with non-existent key
    let sign_result = hsm.sign("non-existent-key", b"data", "ECDSA-SHA256");
    assert!(matches!(sign_result, HsmResult::Error(_)));
    
    let info_result = hsm.get_key_info("non-existent-key");
    assert!(matches!(info_result, HsmResult::Error(_)));
    
    let delete_result = hsm.delete_key("non-existent-key");
    assert!(matches!(delete_result, HsmResult::Error(_)));
}

#[test]
fn test_audit_log_functionality() {
    let mut manager = HsmManager::new();
    let hsm = Box::new(SoftHsm::new());
    
    // Add HSM and perform operations
    manager.add_hsm("audit-test".to_string(), hsm).unwrap();
    
    let config = HsmConfig {
        provider: HsmProvider::SoftHsm,
        connection_string: "test".to_string(),
        slot_id: Some(0),
        pin: Some("1234".to_string()),
        library_path: None,
        timeout_ms: 5000,
    };
    
    manager.initialize_hsm("audit-test", &config).unwrap();
    manager.generate_wallet_key("audit-test-key").unwrap();
    
    // Check audit log
    let audit_log = manager.export_audit_log();
    assert!(audit_log.len() >= 3);
    
    // Verify audit log content
    let operations: Vec<&str> = audit_log.iter().map(|entry| entry.operation.as_str()).collect();
    assert!(operations.contains(&"HSM_ADDED"));
    assert!(operations.contains(&"HSM_INITIALIZED"));
    assert!(operations.contains(&"WALLET_KEY_GENERATED"));
    
    // Test audit log timestamps are in order
    for i in 1..audit_log.len() {
        assert!(audit_log[i].timestamp >= audit_log[i-1].timestamp);
    }
    
    // Test audit log clear functionality
    manager.clear_audit_log();
    let cleared_log = manager.export_audit_log();
    assert_eq!(cleared_log.len(), 0);
}