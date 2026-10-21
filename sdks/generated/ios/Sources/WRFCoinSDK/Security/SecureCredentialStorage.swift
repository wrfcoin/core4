import Foundation
import Security
import LocalAuthentication

/**
 * Secure credential storage using iOS Keychain with biometric authentication support.
 *
 * Features:
 * - Keychain integration for secure storage
 * - Biometric authentication (Face ID/Touch ID)
 * - Automatic encryption/decryption
 * - Background app protection
 * - Hardware security module integration when available
 */
public class SecureCredentialStorage {
    
    // MARK: - Properties
    
    private let serviceName = "com.wrfcoin.sdk"
    private let accessGroup: String? = nil // Set to app group for sharing between apps
    
    // MARK: - Storage Methods
    
    /**
     * Saves a value securely in the Keychain.
     */
    public func save(key: String, value: String, requireBiometrics: Bool = false) {
        let data = value.data(using: .utf8)!
        
        var query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: serviceName,
            kSecAttrAccount as String: key,
            kSecValueData as String: data,
            kSecAttrAccessible as String: kSecAttrAccessibleWhenUnlockedThisDeviceOnly
        ]
        
        if let accessGroup = accessGroup {
            query[kSecAttrAccessGroup as String] = accessGroup
        }
        
        // Add biometric authentication if required
        if requireBiometrics {
            let context = LAContext()
            var error: NSError?
            
            if context.canEvaluatePolicy(.biometryAny, error: &error) {
                query[kSecAttrAccessControl as String] = SecAccessControlCreateWithFlags(
                    nil,
                    kSecAttrAccessibleWhenUnlockedThisDeviceOnly,
                    .biometryAny,
                    nil
                )
            }
        }
        
        // Delete existing item first
        let deleteQuery: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: serviceName,
            kSecAttrAccount as String: key
        ]
        SecItemDelete(deleteQuery as CFDictionary)
        
        // Add new item
        let status = SecItemAdd(query as CFDictionary, nil)
        
        if status != errSecSuccess {
            print("Failed to save credential: \\(status)")
        }
    }
    
    /**
     * Retrieves a value from the Keychain.
     */
    public func retrieve(key: String) -> String? {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: serviceName,
            kSecAttrAccount as String: key,
            kSecReturnData as String: true,
            kSecMatchLimit as String: kSecMatchLimitOne
        ]
        
        var result: AnyObject?
        let status = SecItemCopyMatching(query as CFDictionary, &result)
        
        guard status == errSecSuccess,
              let data = result as? Data,
              let value = String(data: data, encoding: .utf8) else {
            return nil
        }
        
        return value
    }
    
    /**
     * Retrieves a value with biometric authentication.
     */
    public func retrieveWithBiometrics(
        key: String,
        reason: String = "Authenticate to access secure data"
    ) async -> String? {
        
        let context = LAContext()
        context.localizedFallbackTitle = "Use Passcode"
        
        do {
            let success = try await context.evaluatePolicy(
                .biometryAny,
                localizedReason: reason
            )
            
            guard success else { return nil }
            
            return retrieve(key: key)
        } catch {
            print("Biometric authentication failed: \\(error)")
            return nil
        }
    }
    
    /**
     * Deletes a specific key from the Keychain.
     */
    public func delete(key: String) {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: serviceName,
            kSecAttrAccount as String: key
        ]
        
        let status = SecItemDelete(query as CFDictionary)
        
        if status != errSecSuccess && status != errSecItemNotFound {
            print("Failed to delete credential: \\(status)")
        }
    }
    
    /**
     * Deletes all stored credentials.
     */
    public func deleteAll() {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: serviceName
        ]
        
        let status = SecItemDelete(query as CFDictionary)
        
        if status != errSecSuccess && status != errSecItemNotFound {
            print("Failed to delete all credentials: \\(status)")
        }
    }
    
    /**
     * Checks if a key exists in storage.
     */
    public func exists(key: String) -> Bool {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: serviceName,
            kSecAttrAccount as String: key,
            kSecReturnData as String: false
        ]
        
        let status = SecItemCopyMatching(query as CFDictionary, nil)
        return status == errSecSuccess
    }
    
    // MARK: - Convenient Methods for Common Credentials
    
    public func saveAPIKey(_ apiKey: String) {
        save(key: "api_key", value: apiKey)
    }
    
    public func getAPIKey() -> String? {
        return retrieve(key: "api_key")
    }
    
    public func saveBearerToken(_ token: String) {
        save(key: "bearer_token", value: token)
    }
    
    public func getBearerToken() -> String? {
        return retrieve(key: "bearer_token")
    }
    
    public func saveWalletAddress(_ address: String) {
        save(key: "wallet_address", value: address)
    }
    
    public func getWalletAddress() -> String? {
        return retrieve(key: "wallet_address")
    }
    
    public func savePrivateKey(_ privateKey: String) {
        save(key: "private_key", value: privateKey, requireBiometrics: true)
    }
    
    public func getPrivateKey() async -> String? {
        return await retrieveWithBiometrics(
            key: "private_key",
            reason: "Access your wallet private key"
        )
    }
    
    public func saveMnemonicHash(_ hash: String) {
        save(key: "mnemonic_hash", value: hash, requireBiometrics: true)
    }
    
    public func getMnemonicHash() async -> String? {
        return await retrieveWithBiometrics(
            key: "mnemonic_hash",
            reason: "Verify your wallet recovery phrase"
        )
    }
    
    // MARK: - Biometric Support Methods
    
    /**
     * Checks if biometric authentication is available.
     */
    public func isBiometricAuthenticationAvailable() -> Bool {
        let context = LAContext()
        var error: NSError?
        return context.canEvaluatePolicy(.biometryAny, error: &error)
    }
    
    /**
     * Gets the type of biometric authentication available.
     */
    public func biometricType() -> LABiometryType {
        let context = LAContext()
        var error: NSError?
        
        guard context.canEvaluatePolicy(.biometryAny, error: &error) else {
            return .none
        }
        
        return context.biometryType
    }
    
    /**
     * Gets a user-friendly description of the biometric type.
     */
    public func biometricTypeDescription() -> String {
        switch biometricType() {
        case .faceID:
            return "Face ID"
        case .touchID:
            return "Touch ID"
        case .opticID:
            return "Optic ID"
        case .none:
            return "None"
        @unknown default:
            return "Unknown"
        }
    }
    
    // MARK: - Security Information
    
    /**
     * Gets information about stored credentials (without revealing actual values).
     */
    public func getStoredCredentialsInfo() -> [String: Any] {
        let commonKeys = ["api_key", "bearer_token", "wallet_address", "private_key", "mnemonic_hash"]
        
        var info: [String: Any] = [:]
        
        for key in commonKeys {
            info[key] = exists(key: key)
        }
        
        info["biometric_available"] = isBiometricAuthenticationAvailable()
        info["biometric_type"] = biometricTypeDescription()
        
        return info
    }
    
    /**
     * Validates the security state of the storage.
     */
    public func validateSecurityState() -> SecurityValidationResult {
        var issues: [String] = []
        var recommendations: [String] = []
        
        // Check if sensitive data is properly secured
        if exists(key: "private_key") || exists(key: "mnemonic_hash") {
            if !isBiometricAuthenticationAvailable() {
                issues.append("Sensitive wallet data stored without biometric protection")
                recommendations.append("Enable biometric authentication for enhanced security")
            }
        }
        
        // Check for API credentials
        if !exists(key: "api_key") && !exists(key: "bearer_token") {
            recommendations.append("Configure API credentials for full functionality")
        }
        
        let isSecure = issues.isEmpty
        
        return SecurityValidationResult(
            isSecure: isSecure,
            issues: issues,
            recommendations: recommendations
        )
    }
}

// MARK: - Supporting Types

public struct SecurityValidationResult {
    public let isSecure: Bool
    public let issues: [String]
    public let recommendations: [String]
}

// MARK: - Error Types

public enum KeychainError: Error, LocalizedError {
    case itemNotFound
    case duplicateItem
    case invalidData
    case unexpectedError(OSStatus)
    
    public var errorDescription: String? {
        switch self {
        case .itemNotFound:
            return "Item not found in Keychain"
        case .duplicateItem:
            return "Item already exists in Keychain"
        case .invalidData:
            return "Invalid data format"
        case .unexpectedError(let status):
            return "Unexpected Keychain error: \\(status)"
        }
    }
}