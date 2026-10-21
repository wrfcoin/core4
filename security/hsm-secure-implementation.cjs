/**
 * Secure HSM Implementation for WRFCoin
 *
 * This module provides hardware security module (HSM) integration for
 * cryptographic operations. Private keys NEVER leave the HSM - only
 * signatures are returned.
 *
 * Supports:
 * - YubiKey PIV via PKCS#11 (production)
 * - Mock provider for development/CI (when HSM_MOCK_MODE=true)
 *
 * Environment Variables:
 * - HSM_MOCK_MODE: Set to 'true' for development without hardware
 * - PKCS11_LIB: Path to YubiKey PKCS#11 library
 * - HSM_SLOT: PKCS#11 slot number (default: 0)
 * - NODE_ENV: 'production' enforces real HSM usage
 */

const crypto = require('crypto');
const { PKCS11YubiKeyProvider } = require('./pkcs11-yubikey-provider.cjs');
const { PKCS11MockProvider, isMockModeEnabled } = require('./pkcs11-mock-provider.cjs');

/**
 * Secure HSM wrapper class
 * Provides a unified interface for HSM operations
 */
class SecureHSM {
  constructor() {
    this.isInitialized = false;
    this.deviceId = null;
    this.provider = null;
    this.keyHandles = new Map();
    this.auditLog = [];
    this.sessionTimeout = 15 * 60 * 1000; // 15 minutes
    this.activeSessions = new Map();
    this.sessionCleanupTimers = new Map();
  }

  /**
   * Initialize HSM with proper security checks
   * @returns {Promise<boolean>} True if initialization successful
   */
  async initialize() {
    console.log('Initializing Secure HSM...');

    // Check for real HSM device
    const hsmDevice = await this.detectHSMDevice();

    if (!hsmDevice) {
      throw new Error('No HSM device detected. Production requires hardware HSM.');
    }

    this.deviceId = hsmDevice.id;
    this.isInitialized = true;

    this.auditLog.push({
      timestamp: new Date().toISOString(),
      event: 'HSM_INITIALIZED',
      deviceId: this.deviceId,
      isHardware: hsmDevice.isHardware,
      success: true,
    });

    const deviceType = hsmDevice.isHardware ? 'Hardware HSM' : 'Mock HSM (DEVELOPMENT ONLY)';
    console.log(`HSM Device Connected: ${this.deviceId} (${deviceType})`);

    return true;
  }

  /**
   * Detect HSM hardware devices
   * Uses PKCS#11 for real device detection or mock for development
   * @returns {Promise<Object|null>} Device information or null
   */
  async detectHSMDevice() {
    const isProduction = process.env.NODE_ENV === 'production';
    const mockEnabled = isMockModeEnabled();

    // In production, NEVER use mock mode
    if (isProduction && mockEnabled) {
      throw new Error(
        'CRITICAL SECURITY ERROR: HSM_MOCK_MODE cannot be enabled in production. ' +
          'Real hardware HSM is required for production use.'
      );
    }

    // Validate production HSM requirements
    if (isProduction) {
      if (!process.env.PKCS11_LIB && !process.env.HSM_ENDPOINT) {
        throw new Error(
          'CRITICAL SECURITY ERROR: PKCS11_LIB or HSM_ENDPOINT must be configured in production.'
        );
      }

      // Try to connect to real HSM
      try {
        this.provider = new PKCS11YubiKeyProvider();
        const deviceInfo = await this.provider.initialize();

        if (!deviceInfo.isHardware) {
          throw new Error('CRITICAL SECURITY ERROR: Connected device is not a hardware HSM.');
        }

        return deviceInfo;
      } catch (error) {
        throw new Error(`CRITICAL SECURITY ERROR: HSM initialization failed - ${error.message}`);
      }
    }

    // Development/test mode
    if (mockEnabled) {
      console.warn('');
      console.warn('WARNING: Using MOCK HSM - NOT SUITABLE FOR PRODUCTION');
      console.warn('Set NODE_ENV=production and connect a YubiKey for production use');
      console.warn('');

      this.provider = new PKCS11MockProvider();
      return await this.provider.initialize();
    }

    // Try real HSM first, fall back to mock with warning
    try {
      this.provider = new PKCS11YubiKeyProvider();
      const deviceInfo = await this.provider.initialize();
      return deviceInfo;
    } catch (error) {
      console.warn('');
      console.warn(`YubiKey not detected: ${error.message}`);
      console.warn('Falling back to MOCK HSM for development.');
      console.warn('To use real HSM, connect a YubiKey with PIV capability.');
      console.warn('');

      this.provider = new PKCS11MockProvider();
      return await this.provider.initialize();
    }
  }

  /**
   * Validate connection to production HSM hardware
   * @returns {Promise<Object>} HSM connection details
   * @deprecated Use detectHSMDevice() instead
   */
  async validateProductionHSM() {
    // This method is kept for backward compatibility
    const deviceInfo = await this.detectHSMDevice();

    if (!deviceInfo || !deviceInfo.isHardware) {
      throw new Error('Production HSM validation failed: No hardware HSM connected');
    }

    return {
      id: deviceInfo.id,
      type: 'hardware',
      isHardware: true,
      endpoint: deviceInfo.libraryPath || process.env.HSM_ENDPOINT,
      provider: 'yubikey-piv',
      connectedAt: new Date().toISOString(),
      serialNumber: deviceInfo.serialNumber,
      firmwareVersion: deviceInfo.firmwareVersion,
    };
  }

  /**
   * Detect HSM provider from endpoint URL
   * @param {string} endpoint - HSM endpoint URL
   * @returns {string} Provider name
   */
  detectHSMProvider(endpoint) {
    if (!endpoint) return 'pkcs11';

    const lowerEndpoint = endpoint.toLowerCase();
    if (lowerEndpoint.includes('amazonaws.com') || lowerEndpoint.includes('cloudhsm')) {
      return 'aws-cloudhsm';
    }
    if (lowerEndpoint.includes('azure.net') || lowerEndpoint.includes('keyvault')) {
      return 'azure-keyvault';
    }
    if (lowerEndpoint.includes('vault.')) {
      return 'hashicorp-vault';
    }
    if (lowerEndpoint.includes('cloudkms.googleapis.com')) {
      return 'google-cloud-hsm';
    }
    return 'pkcs11';
  }

  /**
   * Generate key pair inside HSM
   * Private key NEVER leaves the HSM
   * @param {string} keyType - Type of key (e.g., 'signing', 'encryption')
   * @param {string} algorithm - Algorithm (default: 'ECDSA-P256')
   * @returns {Promise<Object>} Key information (public key only)
   */
  async generateKeyPair(keyType, algorithm = 'ECDSA-P256') {
    if (!this.isInitialized) {
      throw new Error('HSM not initialized');
    }

    console.log(`Generating ${keyType} key pair in HSM...`);

    // Parse algorithm to get curve
    const curve = algorithm.includes('384') ? 'P-384' : 'P-256';
    const keyId = `${keyType}_${Date.now()}`;

    // Generate key using provider
    const keyInfo = await this.provider.generateKeyPair(keyId, { curve });

    // Store key handle mapping
    this.keyHandles.set(keyInfo.keyHandle, {
      keyType,
      algorithm,
      publicKey: keyInfo.publicKey,
      createdAt: keyInfo.createdAt,
      hsmKeyId: keyInfo.keyId,
    });

    this.auditLog.push({
      timestamp: new Date().toISOString(),
      event: 'KEY_GENERATED',
      keyHandle: keyInfo.keyHandle,
      keyType,
      algorithm,
      success: true,
    });

    return {
      keyHandle: keyInfo.keyHandle,
      publicKey: keyInfo.publicKey.pem || JSON.stringify(keyInfo.publicKey),
      keyType,
      algorithm,
      hsmStored: keyInfo.hsmStored !== false,
    };
  }

  /**
   * Authenticate session for HSM operations
   * @param {Object} credentials - User credentials
   * @returns {Promise<string>} Session ID
   */
  async authenticateSession(credentials) {
    const sessionId = crypto.randomBytes(16).toString('hex');

    // Verify credentials using HSM
    const authResult = await this.verifyCredentials(credentials);

    if (!authResult.valid) {
      this.auditLog.push({
        timestamp: new Date().toISOString(),
        event: 'AUTH_FAILED',
        reason: authResult.reason,
        success: false,
      });
      throw new Error(`Authentication failed: ${authResult.reason}`);
    }

    this.activeSessions.set(sessionId, {
      createdAt: Date.now(),
      credentials: credentials.userId,
      permissions: authResult.permissions,
    });

    // Auto-expire sessions
    const timer = setTimeout(() => {
      this.activeSessions.delete(sessionId);
      this.sessionCleanupTimers.delete(sessionId);
    }, this.sessionTimeout);

    this.sessionCleanupTimers.set(sessionId, timer);

    this.auditLog.push({
      timestamp: new Date().toISOString(),
      event: 'SESSION_AUTHENTICATED',
      sessionId,
      userId: credentials.userId,
      success: true,
    });

    return sessionId;
  }

  /**
   * Sign data using HSM-protected key
   * Private key NEVER leaves the HSM
   * @param {Buffer|string} data - Data to sign
   * @param {string} keyHandle - Handle of the signing key
   * @param {string} sessionId - Authenticated session ID
   * @returns {Promise<Object>} Signature information
   */
  async signWithHSM(data, keyHandle, sessionId) {
    // Verify session
    if (!this.activeSessions.has(sessionId)) {
      throw new Error('Invalid or expired session');
    }

    // Verify key exists
    if (!this.keyHandles.has(keyHandle)) {
      throw new Error('Invalid key handle');
    }

    const keyInfo = this.keyHandles.get(keyHandle);

    console.log(`Signing with HSM-protected ${keyInfo.keyType} key...`);

    // Sign using the provider
    const signResult = await this.provider.sign(data, keyHandle);

    this.auditLog.push({
      timestamp: new Date().toISOString(),
      event: 'DATA_SIGNED',
      keyHandle,
      keyType: keyInfo.keyType,
      dataHash: signResult.dataHash,
      sessionId,
      hsmSigned: signResult.hsmSigned !== false,
      success: true,
    });

    return {
      signature: signResult.signature,
      keyHandle,
      algorithm: keyInfo.algorithm,
      timestamp: signResult.timestamp,
      hsmSigned: signResult.hsmSigned !== false,
    };
  }

  /**
   * Verify credentials with HSM
   * Uses real PIN verification when connected to YubiKey
   * @param {Object} credentials - User credentials
   * @returns {Promise<Object>} Verification result
   */
  async verifyCredentials(credentials) {
    if (!credentials.userId) {
      return { valid: false, reason: 'Missing user ID' };
    }

    if (!credentials.pin) {
      return { valid: false, reason: 'Missing PIN' };
    }

    // Validate PIN format (YubiKey PIV PIN: 6-8 digits)
    if (credentials.pin.length < 4 || credentials.pin.length > 8) {
      return { valid: false, reason: 'PIN must be 4-8 characters' };
    }

    try {
      // Login to HSM with PIN
      const loginResult = await this.provider.login(credentials.pin);

      if (!loginResult.loggedIn) {
        return { valid: false, reason: 'PIN verification failed' };
      }

      return {
        valid: true,
        permissions: ['sign', 'generate_keys', 'verify'],
        authLevel: 'authenticated',
        sessionId: loginResult.sessionId,
      };
    } catch (error) {
      return {
        valid: false,
        reason: error.message,
      };
    }
  }

  /**
   * Get public key for verification
   * @param {string} keyHandle - Handle of the key
   * @returns {string} Public key in PEM format
   */
  getPublicKey(keyHandle) {
    if (!this.keyHandles.has(keyHandle)) {
      throw new Error('Invalid key handle');
    }

    const keyInfo = this.keyHandles.get(keyHandle);
    return keyInfo.publicKey.pem || JSON.stringify(keyInfo.publicKey);
  }

  /**
   * List all key handles (safe to expose)
   * @returns {Array} List of key information
   */
  listKeys() {
    const keys = [];
    for (const [handle, info] of this.keyHandles) {
      keys.push({
        keyHandle: handle,
        keyType: info.keyType,
        algorithm: info.algorithm,
        createdAt: info.createdAt,
        publicKey: info.publicKey.pem || JSON.stringify(info.publicKey),
      });
    }
    return keys;
  }

  /**
   * Get security audit log
   * @returns {Array} Audit log entries
   */
  getAuditLog() {
    // Combine local audit log with provider audit log
    const providerLog = this.provider ? this.provider.getAuditLog() : [];
    return [...this.auditLog, ...providerLog];
  }

  /**
   * Get HSM status information
   * @returns {Object} Status information
   */
  getStatus() {
    const providerStatus = this.provider ? this.provider.getStatus() : null;

    return {
      isInitialized: this.isInitialized,
      deviceId: this.deviceId,
      isHardware: providerStatus?.deviceInfo?.isHardware ?? false,
      isMockMode: providerStatus?.isMockMode ?? false,
      keyCount: this.keyHandles.size,
      activeSessionCount: this.activeSessions.size,
      provider: providerStatus,
    };
  }

  /**
   * Verify a signature
   * @param {Buffer|string} data - Original data
   * @param {string} signature - Signature to verify
   * @param {string} keyHandle - Handle of the key
   * @returns {Promise<boolean>} True if signature is valid
   */
  async verifySignature(data, signature, keyHandle) {
    if (!this.keyHandles.has(keyHandle)) {
      throw new Error('Invalid key handle');
    }

    return await this.provider.verify(data, signature, keyHandle);
  }

  /**
   * Terminate a session
   * @param {string} sessionId - Session ID to terminate
   */
  terminateSession(sessionId) {
    if (this.activeSessions.has(sessionId)) {
      this.activeSessions.delete(sessionId);

      // Clear cleanup timer
      const timer = this.sessionCleanupTimers.get(sessionId);
      if (timer) {
        clearTimeout(timer);
        this.sessionCleanupTimers.delete(sessionId);
      }

      this.auditLog.push({
        timestamp: new Date().toISOString(),
        event: 'SESSION_TERMINATED',
        sessionId,
        success: true,
      });
    }
  }

  /**
   * Secure shutdown
   * @returns {Promise<void>}
   */
  async shutdown() {
    // Clear all sessions
    for (const sessionId of this.activeSessions.keys()) {
      this.terminateSession(sessionId);
    }

    // Finalize provider
    if (this.provider) {
      await this.provider.finalize();
      this.provider = null;
    }

    this.isInitialized = false;

    this.auditLog.push({
      timestamp: new Date().toISOString(),
      event: 'HSM_SHUTDOWN',
      success: true,
    });

    console.log('HSM securely shut down');
  }
}

module.exports = SecureHSM;
