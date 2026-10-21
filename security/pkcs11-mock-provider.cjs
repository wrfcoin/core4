/**
 * Mock PKCS#11 Provider for Development/CI
 *
 * This module provides a software-based mock implementation of the PKCS#11
 * YubiKey provider for development and CI/CD environments where no hardware
 * HSM is available.
 *
 * SECURITY WARNING: This mock provider uses software-based cryptography
 * and should NEVER be used in production environments. Private keys are
 * stored in memory and are NOT protected by hardware security.
 *
 * Enable mock mode by setting: HSM_MOCK_MODE=true
 */

const crypto = require('crypto');

// PIV slot constants (same as real provider for API compatibility)
const PIV_SLOTS = {
  AUTHENTICATION: 0x9a,
  SIGNATURE: 0x9c,
  KEY_MANAGEMENT: 0x9d,
  CARD_AUTHENTICATION: 0x9e,
};

/**
 * Mock PKCS#11 Provider
 * Provides the same interface as PKCS11YubiKeyProvider but uses Node.js crypto
 */
class PKCS11MockProvider {
  constructor() {
    if (process.env.NODE_ENV === 'production') {
      throw new Error('SECURITY: PKCS11MockProvider cannot be instantiated in production.');
    }

    this.isInitialized = false;
    this.isLoggedIn = false;
    this.deviceInfo = null;
    this.keyHandles = new Map();
    this.privateKeys = new Map(); // In-memory private key storage (NOT SECURE!)
    this.auditLog = [];
    this.sessionId = null;

    // Log warning on instantiation
    console.warn('');
    console.warn('='.repeat(70));
    console.warn('  WARNING: MOCK HSM PROVIDER ACTIVE');
    console.warn('  This is a SOFTWARE-BASED mock for development/testing only.');
    console.warn('  Private keys are stored in MEMORY and are NOT hardware-protected.');
    console.warn('  DO NOT USE IN PRODUCTION!');
    console.warn('='.repeat(70));
    console.warn('');
  }

  /**
   * Initialize the mock provider
   * @returns {Promise<Object>} Mock device information
   */
  async initialize() {
    if (this.isInitialized) {
      return this.deviceInfo;
    }

    this.deviceInfo = {
      id: `mock-hsm-${crypto.randomBytes(4).toString('hex')}`,
      type: 'mock-hsm',
      isHardware: false, // Important: indicates this is NOT real hardware
      serialNumber: 'MOCK-12345678',
      label: 'Mock YubiKey (Development)',
      manufacturer: 'WRFCoin Development',
      model: 'MockHSM v1.0',
      firmwareVersion: '0.0.0',
      warning: 'DEVELOPMENT ONLY - NOT FOR PRODUCTION USE',
      mechanisms: ['ECDSA', 'ECDSA_SHA256', 'EC_KEY_PAIR_GEN'],
    };

    this.isInitialized = true;

    this._logAudit('MOCK_DEVICE_INITIALIZED', {
      deviceId: this.deviceInfo.id,
      warning: this.deviceInfo.warning,
    });

    return this.deviceInfo;
  }

  /**
   * Mock login - accepts any PIN in mock mode
   * @param {string} pin - PIN (ignored in mock mode)
   * @returns {Promise<Object>} Session information
   */
  async login(pin) {
    if (!this.isInitialized) {
      throw new Error('Provider not initialized. Call initialize() first.');
    }

    // In mock mode, we accept any PIN but log a warning
    console.warn('MOCK MODE: PIN verification bypassed. Any PIN is accepted.');

    if (!pin || pin.length < 4) {
      throw new Error('PIN must be at least 4 characters (even in mock mode).');
    }

    this.sessionId = crypto.randomBytes(16).toString('hex');
    this.isLoggedIn = true;

    this._logAudit('MOCK_SESSION_OPENED', {
      sessionId: this.sessionId,
      warning: 'PIN verification bypassed in mock mode',
    });

    return {
      sessionId: this.sessionId,
      loggedIn: true,
      slotId: 'mock-slot-0',
      warning: 'Mock session - not cryptographically secure',
    };
  }

  /**
   * Logout from mock session
   */
  async logout() {
    if (this.isLoggedIn) {
      this._logAudit('MOCK_SESSION_CLOSED', {
        sessionId: this.sessionId,
      });

      this.sessionId = null;
      this.isLoggedIn = false;
    }
  }

  /**
   * Generate a mock ECDSA key pair using Node.js crypto
   * @param {string} keyId - Identifier for the key
   * @param {Object} options - Key generation options
   * @returns {Promise<Object>} Key information
   */
  async generateKeyPair(keyId, options = {}) {
    if (!this.isLoggedIn) {
      throw new Error('Not logged in. Call login() first.');
    }

    const curve = options.curve || 'P-256';
    const curveName = curve === 'P-256' ? 'prime256v1' : 'secp384r1';

    // Generate key pair using Node.js crypto
    const { publicKey, privateKey } = crypto.generateKeyPairSync('ec', {
      namedCurve: curveName,
    });

    // Export keys
    const publicKeyDer = publicKey.export({ type: 'spki', format: 'der' });
    const privateKeyDer = privateKey.export({ type: 'pkcs8', format: 'der' });
    const publicKeyPem = publicKey.export({ type: 'spki', format: 'pem' });

    // Generate key handle
    const keyHandle = crypto.randomBytes(16).toString('hex');

    // Store private key in memory (NOT SECURE - only for mock!)
    this.privateKeys.set(keyHandle, {
      privateKey,
      privateKeyDer,
    });

    // Store key metadata
    this.keyHandles.set(keyHandle, {
      keyId,
      curve,
      publicKey,
      publicKeyDer,
      publicKeyPem,
      createdAt: new Date().toISOString(),
    });

    const keyInfo = {
      keyHandle,
      keyId,
      curve,
      publicKey: {
        curve: curveName,
        pem: publicKeyPem,
        der: publicKeyDer.toString('hex'),
      },
      algorithm: `ECDSA-${curve}`,
      createdAt: new Date().toISOString(),
      extractable: true, // Mock keys ARE extractable (for testing)
      hsmStored: false, // Important: indicates not stored in real HSM
      warning: 'Mock key - private key stored in memory, NOT hardware-protected',
    };

    this._logAudit('MOCK_KEY_GENERATED', {
      keyHandle,
      keyId,
      curve,
      warning: 'Private key stored in memory',
    });

    return keyInfo;
  }

  /**
   * Sign data using a mock key
   * @param {Buffer|string} data - Data to sign
   * @param {string} keyHandle - Handle of the key to use
   * @returns {Promise<Object>} Signature information
   */
  async sign(data, keyHandle) {
    if (!this.isLoggedIn) {
      throw new Error('Not logged in. Call login() first.');
    }

    const keyData = this.privateKeys.get(keyHandle);
    if (!keyData) {
      throw new Error(`Key handle not found: ${keyHandle}`);
    }

    const keyInfo = this.keyHandles.get(keyHandle);

    // Hash the data
    const dataBuffer = Buffer.isBuffer(data) ? data : Buffer.from(data);
    const hash = crypto.createHash('sha256').update(dataBuffer).digest();

    // Sign using Node.js crypto
    const sign = crypto.createSign('SHA256');
    sign.update(dataBuffer);
    const signature = sign.sign(keyData.privateKey);

    const signatureInfo = {
      signature: signature.toString('hex'),
      signatureBuffer: signature,
      keyHandle,
      algorithm: 'ECDSA-SHA256',
      dataHash: hash.toString('hex'),
      timestamp: Date.now(),
      hsmSigned: false, // Important: indicates not signed by real HSM
      warning: 'Mock signature - not hardware-protected',
    };

    this._logAudit('MOCK_DATA_SIGNED', {
      keyHandle,
      dataHash: signatureInfo.dataHash,
      warning: 'Software-based signature',
    });

    return signatureInfo;
  }

  /**
   * Verify a signature using a mock key
   * @param {Buffer|string} data - Original data
   * @param {Buffer|string} signature - Signature to verify
   * @param {string} keyHandle - Handle of the key to use
   * @returns {Promise<boolean>} True if signature is valid
   */
  async verify(data, signature, keyHandle) {
    if (!this.isLoggedIn) {
      throw new Error('Not logged in. Call login() first.');
    }

    const keyInfo = this.keyHandles.get(keyHandle);
    if (!keyInfo) {
      throw new Error(`Key handle not found: ${keyHandle}`);
    }

    const dataBuffer = Buffer.isBuffer(data) ? data : Buffer.from(data);
    const sigBuffer = Buffer.isBuffer(signature) ? signature : Buffer.from(signature, 'hex');

    try {
      // Verify using Node.js crypto
      const verify = crypto.createVerify('SHA256');
      verify.update(dataBuffer);
      const isValid = verify.verify(keyInfo.publicKey, sigBuffer);

      this._logAudit('MOCK_SIGNATURE_VERIFIED', {
        keyHandle,
        isValid,
      });

      return isValid;
    } catch (error) {
      return false;
    }
  }

  /**
   * List all mock keys
   * @returns {Promise<Array>} List of key information
   */
  async listKeys() {
    if (!this.isLoggedIn) {
      throw new Error('Not logged in. Call login() first.');
    }

    const keys = [];

    for (const [handle, info] of this.keyHandles) {
      keys.push({
        label: info.keyId,
        keyType: 'EC',
        curve: info.curve,
        handle,
        createdAt: info.createdAt,
        isMock: true,
      });
    }

    return keys;
  }

  /**
   * Delete a mock key
   * @param {string} keyHandle - Handle of the key to delete
   */
  async deleteKey(keyHandle) {
    if (!this.isLoggedIn) {
      throw new Error('Not logged in. Call login() first.');
    }

    if (!this.keyHandles.has(keyHandle)) {
      throw new Error(`Key handle not found: ${keyHandle}`);
    }

    const keyInfo = this.keyHandles.get(keyHandle);

    // Clear private key from memory
    this.privateKeys.delete(keyHandle);
    this.keyHandles.delete(keyHandle);

    this._logAudit('MOCK_KEY_DELETED', {
      keyHandle,
      keyId: keyInfo.keyId,
    });
  }

  /**
   * Get mock device status
   * @returns {Object} Device status
   */
  getStatus() {
    return {
      isInitialized: this.isInitialized,
      isLoggedIn: this.isLoggedIn,
      deviceInfo: this.deviceInfo,
      keyCount: this.keyHandles.size,
      isMockMode: true,
      warning: 'Mock HSM - not suitable for production',
    };
  }

  /**
   * Get audit log entries
   * @returns {Array} Audit log entries
   */
  getAuditLog() {
    return [...this.auditLog];
  }

  /**
   * Cleanup mock provider
   */
  async finalize() {
    await this.logout();

    // Clear all keys from memory
    this.privateKeys.clear();
    this.keyHandles.clear();

    this.isInitialized = false;
    this.deviceInfo = null;

    this._logAudit('MOCK_PROVIDER_FINALIZED', {});
  }

  /**
   * Log an audit entry
   * @private
   */
  _logAudit(event, details) {
    const entry = {
      timestamp: new Date().toISOString(),
      event,
      details,
      isMock: true,
    };

    this.auditLog.push(entry);

    // Keep only last 1000 entries
    if (this.auditLog.length > 1000) {
      this.auditLog = this.auditLog.slice(-1000);
    }
  }
}

/**
 * Check if mock mode is enabled
 * @returns {boolean} True if mock mode is enabled
 */
function isMockModeEnabled() {
  return process.env.HSM_MOCK_MODE === 'true' || process.env.NODE_ENV === 'test';
}

// Export the provider class and helper
module.exports = {
  PKCS11MockProvider,
  PIV_SLOTS,
  isMockModeEnabled,
};
