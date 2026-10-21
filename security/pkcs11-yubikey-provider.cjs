/**
 * PKCS#11 YubiKey Provider for WRFCoin HSM Integration
 *
 * This module provides real PKCS#11 operations for YubiKey PIV devices.
 * Private keys NEVER leave the YubiKey - all cryptographic operations
 * are performed inside the secure hardware.
 *
 * Supported operations:
 * - Device detection and enumeration
 * - PIN-based authentication
 * - ECDSA key generation (P-256, P-384)
 * - Digital signatures using PIV slots
 * - Signature verification
 */

const graphene = require('graphene-pk11');
const crypto = require('crypto');
const path = require('path');
const os = require('os');

// PIV slot constants (YubiKey specific)
const PIV_SLOTS = {
  AUTHENTICATION: 0x9a,
  SIGNATURE: 0x9c,
  KEY_MANAGEMENT: 0x9d,
  CARD_AUTHENTICATION: 0x9e,
};

// PKCS#11 mechanism constants
const MECHANISMS = {
  ECDSA: graphene.MechanismEnum.ECDSA,
  ECDSA_SHA256: graphene.MechanismEnum.ECDSA_SHA256,
  EC_KEY_PAIR_GEN: graphene.MechanismEnum.EC_KEY_PAIR_GEN,
};

// EC curve OIDs
const EC_CURVES = {
  'P-256': Buffer.from([0x06, 0x08, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07]), // secp256r1
  'P-384': Buffer.from([0x06, 0x05, 0x2b, 0x81, 0x04, 0x00, 0x22]), // secp384r1
};

/**
 * PKCS#11 YubiKey Provider
 * Handles all HSM operations using the YubiKey's PKCS#11 interface
 */
class PKCS11YubiKeyProvider {
  constructor() {
    this.module = null;
    this.slots = [];
    this.activeSlot = null;
    this.session = null;
    this.isInitialized = false;
    this.deviceInfo = null;
    this.keyHandles = new Map();
    this.auditLog = [];
  }

  /**
   * Get the default PKCS#11 library path for YubiKey
   * @returns {string} Path to libykcs11
   */
  static getDefaultLibraryPath() {
    const platform = os.platform();

    // Check environment variable first
    if (process.env.PKCS11_LIB) {
      return process.env.PKCS11_LIB;
    }

    // Platform-specific default paths
    switch (platform) {
      case 'linux':
        // Common Linux paths for YubiKey PKCS#11
        const linuxPaths = [
          '/usr/lib/libykcs11.so',
          '/usr/lib/x86_64-linux-gnu/libykcs11.so',
          '/usr/local/lib/libykcs11.so',
          '/usr/lib64/libykcs11.so',
        ];
        for (const p of linuxPaths) {
          try {
            require('fs').accessSync(p);
            return p;
          } catch {
            continue;
          }
        }
        return '/usr/lib/libykcs11.so'; // Default fallback
      case 'darwin':
        return '/usr/local/lib/libykcs11.dylib';
      case 'win32':
        return 'C:\\Program Files\\Yubico\\Yubico PIV Tool\\bin\\libykcs11.dll';
      default:
        throw new Error(`Unsupported platform: ${platform}`);
    }
  }

  /**
   * Initialize the PKCS#11 module and detect YubiKey devices
   * @param {string} libraryPath - Optional path to PKCS#11 library
   * @returns {Promise<Object>} Device information
   */
  async initialize(libraryPath = null) {
    if (this.isInitialized) {
      return this.deviceInfo;
    }

    const libPath = libraryPath || PKCS11YubiKeyProvider.getDefaultLibraryPath();

    try {
      // Initialize PKCS#11 module
      this.module = graphene.Module.load(libPath, 'YubiKey');
      this.module.initialize();

      // Enumerate slots
      this.slots = this.module.getSlots(true); // Only slots with tokens

      if (this.slots.length === 0) {
        throw new Error('No YubiKey devices detected. Please insert a YubiKey with PIV capability.');
      }

      // Use first available slot (or specified by HSM_SLOT env var)
      const slotIndex = parseInt(process.env.HSM_SLOT || '0', 10);
      if (slotIndex >= this.slots.length) {
        throw new Error(`Slot ${slotIndex} not available. Found ${this.slots.length} slot(s).`);
      }

      this.activeSlot = this.slots.items(slotIndex);
      const token = this.activeSlot.getToken();

      // Get device information
      this.deviceInfo = {
        id: `yubikey-${token.serialNumber || 'unknown'}`,
        type: 'yubikey-piv',
        isHardware: true,
        serialNumber: token.serialNumber || 'unknown',
        label: token.label || 'YubiKey PIV',
        manufacturer: token.manufacturerID || 'Yubico',
        model: token.model || 'YubiKey',
        firmwareVersion: `${token.hardwareVersion.major}.${token.hardwareVersion.minor}.${token.hardwareVersion.patch}`,
        libraryPath: libPath,
        slotIndex,
        mechanisms: this._getSupportedMechanisms(),
      };

      this.isInitialized = true;

      this._logAudit('DEVICE_INITIALIZED', {
        deviceId: this.deviceInfo.id,
        serialNumber: this.deviceInfo.serialNumber,
      });

      return this.deviceInfo;
    } catch (error) {
      this._logAudit('INITIALIZATION_FAILED', {
        error: error.message,
        libraryPath: libPath,
      });

      // Provide helpful error messages
      if (error.message.includes('ENOENT') || error.message.includes('cannot open')) {
        throw new Error(
          `YubiKey PKCS#11 library not found at ${libPath}. ` +
            'Please install yubico-piv-tool and ensure libykcs11 is available.'
        );
      }
      throw error;
    }
  }

  /**
   * Get list of supported mechanisms from the slot
   * @private
   */
  _getSupportedMechanisms() {
    if (!this.activeSlot) return [];

    try {
      const mechanisms = this.activeSlot.getMechanisms();
      return Array.from({ length: mechanisms.length }, (_, i) => mechanisms.items(i).name);
    } catch {
      return ['ECDSA', 'ECDSA_SHA256', 'EC_KEY_PAIR_GEN'];
    }
  }

  /**
   * Open a PKCS#11 session and authenticate with PIN
   * @param {string} pin - YubiKey PIV PIN (default: 123456)
   * @returns {Promise<Object>} Session information
   */
  async login(pin) {
    if (!this.isInitialized) {
      throw new Error('Provider not initialized. Call initialize() first.');
    }

    if (this.session) {
      // Already logged in
      return {
        sessionId: this.session.handle.toString(),
        loggedIn: true,
      };
    }

    try {
      // Open a read-write session
      this.session = this.activeSlot.open(graphene.SessionFlag.RW_SESSION | graphene.SessionFlag.SERIAL_SESSION);

      // Login with user PIN
      // Note: PIN is securely handled by the PKCS#11 library
      this.session.login(pin, graphene.UserType.USER);

      const sessionInfo = {
        sessionId: this.session.handle.toString(),
        loggedIn: true,
        slotId: this.activeSlot.handle.toString(),
      };

      this._logAudit('SESSION_OPENED', {
        sessionId: sessionInfo.sessionId,
      });

      return sessionInfo;
    } catch (error) {
      // Handle PIN-related errors
      if (error.message.includes('PIN_INCORRECT')) {
        const retriesLeft = this._getPinRetryCount();
        throw new Error(`Incorrect PIN. ${retriesLeft} retries remaining before lockout.`);
      }
      if (error.message.includes('PIN_LOCKED')) {
        throw new Error('PIN is locked. Please use the PUK to unlock or reset the YubiKey.');
      }
      if (error.message.includes('TOKEN_NOT_PRESENT')) {
        throw new Error('YubiKey was removed. Please reinsert the device.');
      }

      this._logAudit('LOGIN_FAILED', {
        error: error.message,
      });

      throw error;
    }
  }

  /**
   * Get PIN retry count (if available)
   * @private
   */
  _getPinRetryCount() {
    try {
      const token = this.activeSlot.getToken();
      // Most YubiKeys have 3 retries by default
      return token.flags & graphene.TokenFlag.USER_PIN_COUNT_LOW ? 'few' : 'multiple';
    } catch {
      return 'unknown';
    }
  }

  /**
   * Logout and close the PKCS#11 session
   */
  async logout() {
    if (this.session) {
      try {
        this.session.logout();
        this.session.close();
      } catch (error) {
        // Session may already be closed
      }
      this.session = null;

      this._logAudit('SESSION_CLOSED', {});
    }
  }

  /**
   * Generate an ECDSA key pair inside the YubiKey
   * @param {string} keyId - Identifier for the key
   * @param {Object} options - Key generation options
   * @returns {Promise<Object>} Key information (public key only)
   */
  async generateKeyPair(keyId, options = {}) {
    if (!this.session) {
      throw new Error('Not logged in. Call login() first.');
    }

    const curve = options.curve || 'P-256';
    const curveOid = EC_CURVES[curve];
    if (!curveOid) {
      throw new Error(`Unsupported curve: ${curve}. Supported: P-256, P-384`);
    }

    try {
      // Key pair generation template
      const publicKeyTemplate = {
        class: graphene.ObjectClass.PUBLIC_KEY,
        keyType: graphene.KeyType.EC,
        token: true, // Store on token
        verify: true,
        encrypt: false,
        wrap: false,
        label: keyId,
        ecParams: curveOid,
      };

      const privateKeyTemplate = {
        class: graphene.ObjectClass.PRIVATE_KEY,
        keyType: graphene.KeyType.EC,
        token: true, // Store on token
        sign: true,
        decrypt: false,
        unwrap: false,
        sensitive: true,
        extractable: false, // CRITICAL: Private key cannot be extracted
        label: keyId,
      };

      // Generate key pair
      const keys = this.session.generateKeyPair(MECHANISMS.EC_KEY_PAIR_GEN, publicKeyTemplate, privateKeyTemplate);

      // Get public key value
      const publicKeyValue = keys.publicKey.getAttribute({ ecPoint: null }).ecPoint;

      // Store key handle mapping
      const keyHandle = crypto.randomBytes(16).toString('hex');
      this.keyHandles.set(keyHandle, {
        privateKeyHandle: keys.privateKey.handle,
        publicKeyHandle: keys.publicKey.handle,
        keyId,
        curve,
        createdAt: new Date().toISOString(),
      });

      const keyInfo = {
        keyHandle,
        keyId,
        curve,
        publicKey: this._formatPublicKey(publicKeyValue, curve),
        publicKeyRaw: publicKeyValue.toString('hex'),
        algorithm: `ECDSA-${curve}`,
        createdAt: new Date().toISOString(),
        extractable: false,
        hsmStored: true,
      };

      this._logAudit('KEY_GENERATED', {
        keyHandle,
        keyId,
        curve,
        algorithm: keyInfo.algorithm,
      });

      return keyInfo;
    } catch (error) {
      this._logAudit('KEY_GENERATION_FAILED', {
        keyId,
        error: error.message,
      });

      if (error.message.includes('TOKEN_WRITE_PROTECTED')) {
        throw new Error('YubiKey is write-protected. Cannot generate new keys.');
      }
      throw error;
    }
  }

  /**
   * Format EC public key to PEM format
   * @private
   */
  _formatPublicKey(ecPoint, curve) {
    // Create a simple representation - in production, use proper ASN.1 encoding
    const curveName = curve === 'P-256' ? 'prime256v1' : 'secp384r1';

    // For now, return as hex with curve info
    // A full implementation would encode this as proper SPKI
    return {
      curve: curveName,
      x: ecPoint.slice(1, curve === 'P-256' ? 33 : 49).toString('hex'),
      y: ecPoint.slice(curve === 'P-256' ? 33 : 49).toString('hex'),
      uncompressed: ecPoint.toString('hex'),
    };
  }

  /**
   * Sign data using a key stored in the YubiKey
   * @param {Buffer|string} data - Data to sign
   * @param {string} keyHandle - Handle of the key to use
   * @returns {Promise<Object>} Signature information
   */
  async sign(data, keyHandle) {
    if (!this.session) {
      throw new Error('Not logged in. Call login() first.');
    }

    const keyInfo = this.keyHandles.get(keyHandle);
    if (!keyInfo) {
      throw new Error(`Key handle not found: ${keyHandle}`);
    }

    try {
      // Get the private key object
      const privateKey = this.session.getObject(keyInfo.privateKeyHandle);

      // Hash the data first (ECDSA signs the hash)
      const dataBuffer = Buffer.isBuffer(data) ? data : Buffer.from(data);
      const hash = crypto.createHash('sha256').update(dataBuffer).digest();

      // Sign the hash
      const signature = this.session.createSign(MECHANISMS.ECDSA, privateKey).once(hash);

      const signatureInfo = {
        signature: signature.toString('hex'),
        signatureBuffer: signature,
        keyHandle,
        algorithm: 'ECDSA-SHA256',
        dataHash: hash.toString('hex'),
        timestamp: Date.now(),
        hsmSigned: true,
      };

      this._logAudit('DATA_SIGNED', {
        keyHandle,
        dataHash: signatureInfo.dataHash,
        signatureLength: signature.length,
      });

      return signatureInfo;
    } catch (error) {
      this._logAudit('SIGNING_FAILED', {
        keyHandle,
        error: error.message,
      });

      if (error.message.includes('KEY_HANDLE_INVALID')) {
        throw new Error('Key not found on device. It may have been deleted.');
      }
      throw error;
    }
  }

  /**
   * Verify a signature
   * @param {Buffer|string} data - Original data
   * @param {Buffer|string} signature - Signature to verify
   * @param {string} keyHandle - Handle of the key to use
   * @returns {Promise<boolean>} True if signature is valid
   */
  async verify(data, signature, keyHandle) {
    if (!this.session) {
      throw new Error('Not logged in. Call login() first.');
    }

    const keyInfo = this.keyHandles.get(keyHandle);
    if (!keyInfo) {
      throw new Error(`Key handle not found: ${keyHandle}`);
    }

    try {
      // Get the public key object
      const publicKey = this.session.getObject(keyInfo.publicKeyHandle);

      // Hash the data
      const dataBuffer = Buffer.isBuffer(data) ? data : Buffer.from(data);
      const hash = crypto.createHash('sha256').update(dataBuffer).digest();

      // Prepare signature buffer
      const sigBuffer = Buffer.isBuffer(signature) ? signature : Buffer.from(signature, 'hex');

      // Verify the signature
      const isValid = this.session.createVerify(MECHANISMS.ECDSA, publicKey).once(hash, sigBuffer);

      this._logAudit('SIGNATURE_VERIFIED', {
        keyHandle,
        isValid,
      });

      return isValid;
    } catch (error) {
      // Verification failure returns false, not an error
      if (error.message.includes('SIGNATURE_INVALID')) {
        return false;
      }
      throw error;
    }
  }

  /**
   * List all keys stored on the YubiKey
   * @returns {Promise<Array>} List of key information
   */
  async listKeys() {
    if (!this.session) {
      throw new Error('Not logged in. Call login() first.');
    }

    const keys = [];

    try {
      // Find all public keys
      const publicKeys = this.session.find({
        class: graphene.ObjectClass.PUBLIC_KEY,
      });

      publicKeys.forEach((key) => {
        try {
          const attrs = key.getAttribute({
            label: null,
            keyType: null,
            id: null,
          });

          keys.push({
            label: attrs.label,
            keyType: attrs.keyType === graphene.KeyType.EC ? 'EC' : 'RSA',
            id: attrs.id ? attrs.id.toString('hex') : null,
            handle: key.handle.toString(),
          });
        } catch {
          // Skip keys we can't read
        }
      });
    } catch (error) {
      this._logAudit('LIST_KEYS_FAILED', {
        error: error.message,
      });
    }

    return keys;
  }

  /**
   * Delete a key from the YubiKey
   * @param {string} keyHandle - Handle of the key to delete
   */
  async deleteKey(keyHandle) {
    if (!this.session) {
      throw new Error('Not logged in. Call login() first.');
    }

    const keyInfo = this.keyHandles.get(keyHandle);
    if (!keyInfo) {
      throw new Error(`Key handle not found: ${keyHandle}`);
    }

    try {
      // Delete both public and private keys
      const privateKey = this.session.getObject(keyInfo.privateKeyHandle);
      const publicKey = this.session.getObject(keyInfo.publicKeyHandle);

      privateKey.destroy();
      publicKey.destroy();

      this.keyHandles.delete(keyHandle);

      this._logAudit('KEY_DELETED', {
        keyHandle,
        keyId: keyInfo.keyId,
      });
    } catch (error) {
      this._logAudit('KEY_DELETION_FAILED', {
        keyHandle,
        error: error.message,
      });
      throw error;
    }
  }

  /**
   * Get device status and health information
   * @returns {Object} Device status
   */
  getStatus() {
    return {
      isInitialized: this.isInitialized,
      isLoggedIn: !!this.session,
      deviceInfo: this.deviceInfo,
      keyCount: this.keyHandles.size,
      slotCount: this.slots.length,
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
   * Cleanup and close all resources
   */
  async finalize() {
    await this.logout();

    if (this.module) {
      try {
        this.module.finalize();
      } catch {
        // Module may already be finalized
      }
      this.module = null;
    }

    this.isInitialized = false;
    this.deviceInfo = null;
    this.keyHandles.clear();

    this._logAudit('PROVIDER_FINALIZED', {});
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
    };

    this.auditLog.push(entry);

    // Keep only last 1000 entries
    if (this.auditLog.length > 1000) {
      this.auditLog = this.auditLog.slice(-1000);
    }
  }
}

// Export the provider class and constants
module.exports = {
  PKCS11YubiKeyProvider,
  PIV_SLOTS,
  MECHANISMS,
  EC_CURVES,
};
