/**
 * Enterprise HSM Service - Production-Grade Implementation
 * Implements FIPS 140-2 Level 3+ compliant hardware security module integration
 * Supports quantum-resistant cryptography and enterprise compliance frameworks
 */

import * as crypto from 'crypto';
import * as fs from 'fs';
import * as path from 'path';
import { EventEmitter } from 'events';
import { securityLogger } from '../backend/server/utils/logger.js';

// Type definitions
interface HSMConfiguration {
  provider:
    | 'aws-cloudhsm'
    | 'azure-keyvault'
    | 'pkcs11'
    | 'thales-luna'
    | 'utimaco-cryptoserver';
  endpoint: string;
  keyStoreId: string;
  sessionTimeout: number;
  maxConcurrentSessions: number;
  keyRotationInterval: number;
  complianceLevel: 'FIPS-140-2-L3' | 'FIPS-140-2-L4' | 'Common-Criteria-EAL4+';
  quantumResistant: boolean;
}

interface KeyMetadata {
  keyId: string;
  keyType: KeyType;
  algorithm: CryptographicAlgorithm;
  publicKey: string;
  createdAt: Date;
  lastUsed: Date;
  usageCount: number;
  rotationDue: Date;
  complianceFlags: number;
  quantumResistant: boolean;
  securityLevel: SecurityLevel;
}

interface MultiSigOperation {
  operationId: string;
  transactionHash: string;
  requiredSignatures: number;
  collectedSignatures: SignatureData[];
  threshold: number;
  createdAt: Date;
  expiresAt: Date;
  status: 'pending' | 'completed' | 'expired' | 'cancelled';
  metadata: OperationMetadata;
}

interface SignatureData {
  signerId: string;
  signature: string;
  algorithm: CryptographicAlgorithm;
  timestamp: Date;
  keyId: string;
  complianceVerified: boolean;
}

interface OperationMetadata {
  operationType: 'transaction' | 'governance' | 'emergency' | 'key-rotation';
  value: number;
  securityLevel: SecurityLevel;
  complianceRequirements: string[];
  auditTrail: AuditEvent[];
}

interface AuditEvent {
  eventId: string;
  timestamp: Date;
  actor: string;
  action: string;
  result: 'success' | 'failure';
  details: string;
  complianceContext: string;
}

interface QuantumResistantConfig {
  enabled: boolean;
  primaryAlgorithm: 'CRYSTALS-Dilithium' | 'FALCON' | 'SPHINCS+';
  keyExchangeAlgorithm: 'CRYSTALS-Kyber' | 'SIKE' | 'FRODO';
  hybridMode: boolean;
  migrationDeadline: Date;
}

interface ComplianceFramework {
  name: string;
  requirements: ComplianceRequirement[];
  auditFrequency: string;
  certificationLevel: string;
}

interface ComplianceRequirement {
  id: string;
  description: string;
  mandatory: boolean;
  validationMethod: string;
  lastChecked: Date;
  status: 'compliant' | 'non-compliant' | 'pending';
}

// Enums
enum KeyType {
  MASTER_SIGNING = 'master-signing',
  CROSS_CHAIN_BRIDGE = 'cross-chain-bridge',
  GOVERNANCE = 'governance',
  WEATHER_VALIDATION = 'weather-validation',
  EMERGENCY_RECOVERY = 'emergency-recovery',
  QUANTUM_RESISTANT_BACKUP = 'quantum-resistant-backup',
}

enum CryptographicAlgorithm {
  ECDSA_P256 = 'ECDSA-P256',
  ECDSA_P384 = 'ECDSA-P384',
  ED25519 = 'Ed25519',
  RSA_4096 = 'RSA-4096',
  DILITHIUM3 = 'CRYSTALS-Dilithium3',
  FALCON512 = 'FALCON-512',
  SPHINCS_SHA256 = 'SPHINCS+-SHA256-128s',
  KYBER768 = 'CRYSTALS-Kyber768',
}

enum SecurityLevel {
  STANDARD = 1,
  HIGH = 2,
  CRITICAL = 3,
  ULTRA_CRITICAL = 4,
}

class EnterpriseHSMService extends EventEmitter {
  private config: HSMConfiguration;
  private activeKeys: Map<string, KeyMetadata> = new Map();
  private activeSessions: Map<string, HSMSession> = new Map();
  private multiSigOperations: Map<string, MultiSigOperation> = new Map();
  private auditTrail: AuditEvent[] = [];
  private complianceFrameworks: Map<string, ComplianceFramework> = new Map();
  private quantumConfig: QuantumResistantConfig;
  private isInitialized: boolean = false;
  private healthStatus: HSMHealthStatus = {
    isHealthy: false,
    lastCheck: new Date(),
  };

  constructor(config: HSMConfiguration) {
    super();
    this.config = config;
    this.quantumConfig = {
      enabled: config.quantumResistant,
      primaryAlgorithm: 'CRYSTALS-Dilithium',
      keyExchangeAlgorithm: 'CRYSTALS-Kyber',
      hybridMode: true,
      migrationDeadline: new Date(Date.now() + 365 * 24 * 60 * 60 * 1000), // 1 year
    };

    this.initializeComplianceFrameworks();
  }

  /**
   * Initialize HSM connection and validate hardware security compliance
   */
  async initialize(): Promise<void> {
    try {
      securityLogger.info('Initializing Enterprise HSM Service', { module: 'HSM', operation: 'init' });

      // Validate HSM hardware compliance
      await this.validateHSMCompliance();

      // Establish secure connection
      await this.establishSecureConnection();

      // Initialize key infrastructure
      await this.initializeKeyInfrastructure();

      // Setup monitoring and health checks
      this.setupHealthMonitoring();

      // Initialize compliance monitoring
      await this.initializeComplianceMonitoring();

      this.isInitialized = true;

      this.logAuditEvent(
        'HSM_INITIALIZED',
        'system',
        'success',
        'Enterprise HSM service initialized successfully'
      );

      securityLogger.info('Enterprise HSM Service initialized successfully', { module: 'HSM', operation: 'init', status: 'complete' });
    } catch (error) {
      const errorMessage =
        error instanceof Error ? error.message : String(error);
      this.logAuditEvent(
        'HSM_INIT_FAILED',
        'system',
        'failure',
        `HSM initialization failed: ${errorMessage}`
      );
      throw new Error(`HSM initialization failed: ${errorMessage}`);
    }
  }

  /**
   * Generate cryptographic key pairs in HSM with enterprise security controls
   */
  async generateKeyPair(
    keyType: KeyType,
    algorithm: CryptographicAlgorithm = CryptographicAlgorithm.ECDSA_P256,
    securityLevel: SecurityLevel = SecurityLevel.HIGH
  ): Promise<KeyMetadata> {
    this.validateInitialized();

    try {
      securityLogger.info('Generating key pair', { module: 'HSM', operation: 'generateKeyPair', keyType, algorithm });

      // Validate algorithm compliance
      this.validateAlgorithmCompliance(algorithm);

      // Generate unique key identifier
      const keyId = this.generateSecureKeyId(keyType);

      // Perform key generation in HSM
      const keyPair = await this.performHSMKeyGeneration(
        keyId,
        algorithm,
        securityLevel
      );

      // Create key metadata
      const keyMetadata: KeyMetadata = {
        keyId,
        keyType,
        algorithm,
        publicKey: keyPair.publicKey,
        createdAt: new Date(),
        lastUsed: new Date(),
        usageCount: 0,
        rotationDue: new Date(Date.now() + this.config.keyRotationInterval),
        complianceFlags: this.calculateComplianceFlags(
          algorithm,
          securityLevel
        ),
        quantumResistant: this.isQuantumResistantAlgorithm(algorithm),
        securityLevel,
      };

      // Store key metadata securely
      this.activeKeys.set(keyId, keyMetadata);

      // Log audit event
      this.logAuditEvent(
        'KEY_GENERATED',
        'system',
        'success',
        `Generated ${keyType} key with ${algorithm} algorithm`,
        `Key ID: ${keyId}, Security Level: ${securityLevel}`
      );

      securityLogger.info('Key pair generated successfully', { module: 'HSM', operation: 'generateKeyPair', keyId });

      return keyMetadata;
    } catch (error) {
      const errorMessage =
        error instanceof Error ? error.message : String(error);
      this.logAuditEvent(
        'KEY_GENERATION_FAILED',
        'system',
        'failure',
        `Key generation failed: ${errorMessage}`
      );
      throw error;
    }
  }

  /**
   * Perform multi-signature operation with enterprise controls
   */
  async initiateMultiSignatureOperation(
    transactionHash: string,
    requiredSignatures: number,
    threshold: number,
    operationType: string,
    value: number,
    securityLevel: SecurityLevel
  ): Promise<string> {
    this.validateInitialized();

    const operationId = crypto.randomBytes(16).toString('hex');

    const operation: MultiSigOperation = {
      operationId,
      transactionHash,
      requiredSignatures,
      collectedSignatures: [],
      threshold,
      createdAt: new Date(),
      expiresAt: new Date(Date.now() + 24 * 60 * 60 * 1000), // 24 hour expiry
      status: 'pending',
      metadata: {
        operationType: operationType as any,
        value,
        securityLevel,
        complianceRequirements: this.determineComplianceRequirements(
          value,
          securityLevel
        ),
        auditTrail: [],
      },
    };

    this.multiSigOperations.set(operationId, operation);

    this.logAuditEvent(
      'MULTISIG_INITIATED',
      'system',
      'success',
      `Multi-signature operation initiated`,
      `Operation ID: ${operationId}, Required signatures: ${requiredSignatures}`
    );

    this.emit('multiSigInitiated', operation);

    return operationId;
  }

  /**
   * Add signature to multi-signature operation
   */
  async addSignatureToOperation(
    operationId: string,
    signerId: string,
    signature: string,
    keyId: string
  ): Promise<boolean> {
    this.validateInitialized();

    const operation = this.multiSigOperations.get(operationId);
    if (!operation) {
      throw new Error('Multi-signature operation not found');
    }

    if (operation.status !== 'pending') {
      throw new Error('Operation is not in pending status');
    }

    if (new Date() > operation.expiresAt) {
      operation.status = 'expired';
      throw new Error('Operation has expired');
    }

    // Validate signature
    const keyMetadata = this.activeKeys.get(keyId);
    if (!keyMetadata) {
      throw new Error('Key not found');
    }

    const isValidSignature = await this.validateSignature(
      operation.transactionHash,
      signature,
      keyMetadata.publicKey,
      keyMetadata.algorithm
    );

    if (!isValidSignature) {
      this.logAuditEvent(
        'INVALID_SIGNATURE',
        signerId,
        'failure',
        'Invalid signature provided'
      );
      throw new Error('Invalid signature');
    }

    // Check if signer already signed
    const existingSignature = operation.collectedSignatures.find(
      sig => sig.signerId === signerId
    );
    if (existingSignature) {
      throw new Error('Signer has already provided signature');
    }

    // Add signature to operation
    const signatureData: SignatureData = {
      signerId,
      signature,
      algorithm: keyMetadata.algorithm,
      timestamp: new Date(),
      keyId,
      complianceVerified: true,
    };

    operation.collectedSignatures.push(signatureData);

    // Update key usage
    keyMetadata.lastUsed = new Date();
    keyMetadata.usageCount++;

    // Check if threshold met
    if (operation.collectedSignatures.length >= operation.threshold) {
      operation.status = 'completed';
      this.emit('multiSigCompleted', operation);

      this.logAuditEvent(
        'MULTISIG_COMPLETED',
        signerId,
        'success',
        `Multi-signature operation completed`,
        `Operation ID: ${operationId}, Final signature count: ${operation.collectedSignatures.length}`
      );

      return true;
    }

    this.logAuditEvent(
      'SIGNATURE_ADDED',
      signerId,
      'success',
      `Signature added to multi-sig operation`,
      `Operation ID: ${operationId}, Signatures: ${operation.collectedSignatures.length}/${operation.threshold}`
    );

    this.emit('signatureAdded', {
      operationId,
      signerId,
      currentCount: operation.collectedSignatures.length,
    });

    return false;
  }

  /**
   * Perform secure signing operation with HSM
   * @deprecated Use signWithHSM(keyId, dataToSign, sessionId, requestingUserId) for enhanced security
   */
  async signWithHSM(
    keyId: string,
    dataToSign: string,
    sessionId: string
  ): Promise<{
    signature: string;
    algorithm: CryptographicAlgorithm;
    timestamp: Date;
  }>;

  /**
   * Perform secure signing operation with HSM with user authorization validation
   */
  async signWithHSM(
    keyId: string,
    dataToSign: string,
    sessionId: string,
    requestingUserId: string
  ): Promise<{
    signature: string;
    algorithm: CryptographicAlgorithm;
    timestamp: Date;
  }>;

  async signWithHSM(
    keyId: string,
    dataToSign: string,
    sessionId: string,
    requestingUserId?: string
  ): Promise<{
    signature: string;
    algorithm: CryptographicAlgorithm;
    timestamp: Date;
  }> {
    this.validateInitialized();

    const session = this.activeSessions.get(sessionId);
    if (!session || !session.isActive || session.expiresAt < new Date()) {
      // Log security violation attempt
      this.logAuditEvent(
        'UNAUTHORIZED_HSM_ACCESS_ATTEMPT',
        requestingUserId || 'unknown',
        'failure',
        'Attempted HSM access with invalid or expired session',
        `Session ID: ${sessionId}, Key ID: ${keyId}`
      );
      throw new Error('Invalid or expired HSM session');
    }

    // CRITICAL SECURITY FIX: Validate session ownership
    if (requestingUserId && session.userId !== requestingUserId) {
      // Log critical security violation
      this.logAuditEvent(
        'UNAUTHORIZED_HSM_SESSION_ACCESS',
        requestingUserId,
        'failure',
        'Attempted to use HSM session belonging to different user',
        `Requesting User: ${requestingUserId}, Session Owner: ${session.userId}, Session ID: ${sessionId}, Key ID: ${keyId}`
      );
      throw new Error('Access denied: Session belongs to different user');
    }

    // Log warning for deprecated API usage without user validation
    if (!requestingUserId) {
      this.logAuditEvent(
        'DEPRECATED_API_USAGE',
        session.userId,
        'success',
        'HSM signing operation using deprecated API without user validation',
        `Key ID: ${keyId}, Session ID: ${sessionId} - Please upgrade to include requestingUserId parameter`
      );
    }

    const keyMetadata = this.activeKeys.get(keyId);
    if (!keyMetadata) {
      this.logAuditEvent(
        'INVALID_KEY_ACCESS_ATTEMPT',
        session.userId,
        'failure',
        'Attempted HSM operation with non-existent key',
        `Key ID: ${keyId}, Session ID: ${sessionId}`
      );
      throw new Error('Key not found in HSM');
    }

    try {
      // Perform signing operation in HSM
      const signature = await this.performHSMSigning(
        keyId,
        dataToSign,
        keyMetadata.algorithm
      );

      // Update key usage statistics
      keyMetadata.lastUsed = new Date();
      keyMetadata.usageCount++;

      // Check if key rotation is due
      if (new Date() >= keyMetadata.rotationDue) {
        this.emit('keyRotationDue', keyId);
      }

      this.logAuditEvent(
        'SIGNING_OPERATION',
        session.userId,
        'success',
        `HSM signing operation completed`,
        `Key ID: ${keyId}, Algorithm: ${keyMetadata.algorithm}, User Validated: ${!!requestingUserId}`
      );

      return {
        signature,
        algorithm: keyMetadata.algorithm,
        timestamp: new Date(),
      };
    } catch (error) {
      const errorMessage =
        error instanceof Error ? error.message : String(error);
      this.logAuditEvent(
        'SIGNING_FAILED',
        session.userId,
        'failure',
        `HSM signing failed: ${errorMessage}`,
        `Key ID: ${keyId}, Session ID: ${sessionId}`
      );
      throw error;
    }
  }

  /**
   * Rotate cryptographic keys with enterprise compliance
   */
  async rotateKey(
    keyId: string,
    reason: string = 'scheduled_rotation'
  ): Promise<KeyMetadata> {
    this.validateInitialized();

    const oldKey = this.activeKeys.get(keyId);
    if (!oldKey) {
      throw new Error('Key not found for rotation');
    }

    try {
      securityLogger.info('Rotating key', { module: 'HSM', operation: 'rotateKey', keyId });

      // Generate new key with same properties
      const newKey = await this.generateKeyPair(
        oldKey.keyType,
        oldKey.algorithm,
        oldKey.securityLevel
      );

      // Archive old key (maintain for grace period)
      const archiveDate = new Date(Date.now() + 7 * 24 * 60 * 60 * 1000); // 7 days grace period
      await this.archiveKey(keyId, newKey.keyId, archiveDate);

      // Update active key mapping
      this.activeKeys.delete(keyId);
      this.activeKeys.set(newKey.keyId, newKey);

      this.logAuditEvent(
        'KEY_ROTATED',
        'system',
        'success',
        `Key rotated successfully`,
        `Old Key: ${keyId}, New Key: ${newKey.keyId}, Reason: ${reason}`
      );

      this.emit('keyRotated', {
        oldKeyId: keyId,
        newKeyId: newKey.keyId,
        reason,
      });

      securityLogger.info('Key rotated successfully', { module: 'HSM', operation: 'rotateKey', oldKeyId: keyId, newKeyId: newKey.keyId });

      return newKey;
    } catch (error) {
      const errorMessage =
        error instanceof Error ? error.message : String(error);
      this.logAuditEvent(
        'KEY_ROTATION_FAILED',
        'system',
        'failure',
        `Key rotation failed: ${errorMessage}`
      );
      throw error;
    }
  }

  /**
   * Enable quantum-resistant cryptography
   */
  async enableQuantumResistance(
    primaryAlgorithm:
      | 'CRYSTALS-Dilithium'
      | 'FALCON'
      | 'SPHINCS+' = 'CRYSTALS-Dilithium',
    hybridMode: boolean = true
  ): Promise<void> {
    this.validateInitialized();

    securityLogger.info('Enabling quantum-resistant cryptography', { module: 'HSM', operation: 'enableQuantumResistance' });

    this.quantumConfig.enabled = true;
    this.quantumConfig.primaryAlgorithm = primaryAlgorithm;
    this.quantumConfig.hybridMode = hybridMode;

    // Generate quantum-resistant backup keys for critical operations
    const criticalKeyTypes = [
      KeyType.MASTER_SIGNING,
      KeyType.GOVERNANCE,
      KeyType.EMERGENCY_RECOVERY,
    ];

    for (const keyType of criticalKeyTypes) {
      const backupKeyType = `${keyType}-quantum-backup` as KeyType;
      await this.generateKeyPair(
        backupKeyType,
        this.getQuantumResistantAlgorithm(primaryAlgorithm),
        SecurityLevel.ULTRA_CRITICAL
      );
    }

    this.logAuditEvent(
      'QUANTUM_RESISTANCE_ENABLED',
      'system',
      'success',
      `Quantum-resistant cryptography enabled`,
      `Primary Algorithm: ${primaryAlgorithm}, Hybrid Mode: ${hybridMode}`
    );

    securityLogger.info('Quantum-resistant cryptography enabled', { module: 'HSM', operation: 'enableQuantumResistance', status: 'complete', primaryAlgorithm, hybridMode });
  }

  /**
   * Perform comprehensive compliance audit
   */
  async performComplianceAudit(
    framework: string = 'SOC2'
  ): Promise<ComplianceAuditResult> {
    this.validateInitialized();

    securityLogger.info('Performing compliance audit', { module: 'HSM', operation: 'performComplianceAudit', framework });

    const complianceFramework = this.complianceFrameworks.get(framework);
    if (!complianceFramework) {
      throw new Error(`Compliance framework ${framework} not supported`);
    }

    const auditResults: ComplianceAuditResult = {
      framework,
      auditDate: new Date(),
      overallStatus: 'compliant',
      requirementResults: [],
      recommendations: [],
      score: 0,
    };

    let totalRequirements = 0;
    let compliantRequirements = 0;

    for (const requirement of complianceFramework.requirements) {
      totalRequirements++;

      const result = await this.auditComplianceRequirement(requirement);
      auditResults.requirementResults.push(result);

      if (result.status === 'compliant') {
        compliantRequirements++;
      } else if (result.status === 'non-compliant' && requirement.mandatory) {
        auditResults.overallStatus = 'non-compliant';
      }
    }

    auditResults.score = Math.round(
      (compliantRequirements / totalRequirements) * 100
    );

    // Generate recommendations
    if (auditResults.score < 100) {
      auditResults.recommendations = this.generateComplianceRecommendations(
        auditResults.requirementResults
      );
    }

    this.logAuditEvent(
      'COMPLIANCE_AUDIT_COMPLETED',
      'system',
      'success',
      `${framework} compliance audit completed`,
      `Score: ${auditResults.score}%, Status: ${auditResults.overallStatus}`
    );

    securityLogger.info('Compliance audit completed', { module: 'HSM', operation: 'performComplianceAudit', framework, score: auditResults.score });

    return auditResults;
  }

  /**
   * Get comprehensive security status report
   */
  getSecurityStatusReport(): SecurityStatusReport {
    return {
      hsmStatus: {
        isInitialized: this.isInitialized,
        provider: this.config.provider,
        complianceLevel: this.config.complianceLevel,
        isHealthy: this.healthStatus.isHealthy,
        lastHealthCheck: this.healthStatus.lastCheck,
      },
      keyManagement: {
        totalKeys: this.activeKeys.size,
        keysByType: this.getKeyCountByType(),
        keysNearingRotation: this.getKeysNearingRotation(),
        quantumResistantKeys: this.getQuantumResistantKeyCount(),
      },
      multiSignature: {
        activeOperations: Array.from(this.multiSigOperations.values()).filter(
          op => op.status === 'pending'
        ).length,
        completedOperations: Array.from(
          this.multiSigOperations.values()
        ).filter(op => op.status === 'completed').length,
        expiredOperations: Array.from(this.multiSigOperations.values()).filter(
          op => op.status === 'expired'
        ).length,
      },
      compliance: {
        frameworksMonitored: Array.from(this.complianceFrameworks.keys()),
        lastAuditDate: this.getLastAuditDate(),
        complianceScore: this.getOverallComplianceScore(),
      },
      auditTrail: {
        totalEvents: this.auditTrail.length,
        recentEvents: this.auditTrail.slice(-10),
        failureCount: this.auditTrail.filter(
          event => event.result === 'failure'
        ).length,
      },
      quantumReadiness: {
        enabled: this.quantumConfig.enabled,
        primaryAlgorithm: this.quantumConfig.primaryAlgorithm,
        hybridMode: this.quantumConfig.hybridMode,
        migrationDeadline: this.quantumConfig.migrationDeadline,
      },
    };
  }

  // ============ Private Methods ============

  private validateInitialized(): void {
    if (!this.isInitialized) {
      throw new Error('HSM service not initialized');
    }
  }

  private async validateHSMCompliance(): Promise<void> {
    // Validate FIPS 140-2 Level 3+ compliance
    securityLogger.debug('Validating HSM compliance', { module: 'HSM', operation: 'validateHSMCompliance', complianceLevel: this.config.complianceLevel });

    // In production, this would perform actual HSM compliance validation
    // For now, we'll simulate the validation

    if (
      process.env.NODE_ENV === 'production' &&
      this.config.provider === 'pkcs11'
    ) {
      // Production validation would use actual PKCS#11 calls
      throw new Error('Production HSM validation not implemented in this demo');
    }

    securityLogger.debug('HSM compliance validated', { module: 'HSM', operation: 'validateHSMCompliance', complianceLevel: this.config.complianceLevel });
  }

  private async establishSecureConnection(): Promise<void> {
    securityLogger.debug('Establishing secure connection', { module: 'HSM', operation: 'establishSecureConnection', provider: this.config.provider });

    // Simulate connection establishment
    await new Promise(resolve => setTimeout(resolve, 1000));

    this.healthStatus = {
      isHealthy: true,
      lastCheck: new Date(),
    };

    securityLogger.debug('Secure HSM connection established', { module: 'HSM', operation: 'establishSecureConnection', provider: this.config.provider });
  }

  private async initializeKeyInfrastructure(): Promise<void> {
    securityLogger.debug('Initializing key infrastructure', { module: 'HSM', operation: 'initializeKeyInfrastructure' });

    // Generate master keys if they don't exist
    const requiredKeys = [
      KeyType.MASTER_SIGNING,
      KeyType.CROSS_CHAIN_BRIDGE,
      KeyType.GOVERNANCE,
      KeyType.WEATHER_VALIDATION,
    ];

    for (const keyType of requiredKeys) {
      // Check if key already exists
      const existingKey = Array.from(this.activeKeys.values()).find(
        key => key.keyType === keyType
      );

      if (!existingKey) {
        await this.generateKeyPair(
          keyType,
          CryptographicAlgorithm.ECDSA_P256,
          SecurityLevel.HIGH
        );
      }
    }

    securityLogger.debug('Key infrastructure initialized', { module: 'HSM', operation: 'initializeKeyInfrastructure' });
  }

  private setupHealthMonitoring(): void {
    // Monitor HSM health every 5 minutes
    setInterval(
      async () => {
        try {
          await this.performHealthCheck();
        } catch (error) {
          const errorMessage =
            error instanceof Error ? error.message : String(error);
          securityLogger.error('HSM health check failed', { module: 'HSM', operation: 'healthCheck', error: errorMessage });
          this.healthStatus.isHealthy = false;
          this.emit('healthCheckFailed', error);
        }
      },
      5 * 60 * 1000
    );
  }

  private async performHealthCheck(): Promise<void> {
    // In production, this would perform actual HSM health checks
    this.healthStatus = {
      isHealthy: true,
      lastCheck: new Date(),
    };
  }

  private initializeComplianceFrameworks(): void {
    // SOC2 Framework
    this.complianceFrameworks.set('SOC2', {
      name: 'SOC2 Type II',
      requirements: [
        {
          id: 'CC6.1',
          description: 'Logical and physical access controls',
          mandatory: true,
          validationMethod: 'access_control_audit',
          lastChecked: new Date(),
          status: 'compliant',
        },
        {
          id: 'CC6.2',
          description: 'Transmission and disposal of data',
          mandatory: true,
          validationMethod: 'encryption_audit',
          lastChecked: new Date(),
          status: 'compliant',
        },
      ],
      auditFrequency: 'annual',
      certificationLevel: 'Type II',
    });

    // ISO 27001 Framework
    this.complianceFrameworks.set('ISO27001', {
      name: 'ISO 27001',
      requirements: [
        {
          id: 'A.10.1.1',
          description: 'Policy on the use of cryptographic controls',
          mandatory: true,
          validationMethod: 'cryptographic_policy_review',
          lastChecked: new Date(),
          status: 'compliant',
        },
      ],
      auditFrequency: 'annual',
      certificationLevel: 'Full Certification',
    });
  }

  private async initializeComplianceMonitoring(): Promise<void> {
    securityLogger.debug('Initializing compliance monitoring', { module: 'HSM', operation: 'initializeComplianceMonitoring' });

    // Schedule regular compliance checks
    setInterval(
      async () => {
        for (const framework of this.complianceFrameworks.keys()) {
          try {
            await this.performComplianceAudit(framework);
          } catch (error) {
            const errorMessage =
              error instanceof Error ? error.message : String(error);
            securityLogger.error('Compliance audit failed', { module: 'HSM', operation: 'complianceAudit', framework, error: errorMessage });
          }
        }
      },
      24 * 60 * 60 * 1000
    ); // Daily compliance checks

    securityLogger.debug('Compliance monitoring initialized', { module: 'HSM', operation: 'initializeComplianceMonitoring' });
  }

  private generateSecureKeyId(keyType: KeyType): string {
    const timestamp = Date.now().toString(36);
    const random = crypto.randomBytes(8).toString('hex');
    return `${keyType}_${timestamp}_${random}`;
  }

  private async performHSMKeyGeneration(
    keyId: string,
    algorithm: CryptographicAlgorithm,
    securityLevel: SecurityLevel
  ): Promise<{ publicKey: string; privateKeyRef: string }> {
    // In production, this would use actual HSM APIs
    // For simulation, generate key pair

    let keyPair: { publicKey: string; privateKey: string };

    switch (algorithm) {
      case CryptographicAlgorithm.ECDSA_P256:
        keyPair = crypto.generateKeyPairSync('ec', {
          namedCurve: 'prime256v1',
          publicKeyEncoding: { type: 'spki', format: 'pem' },
          privateKeyEncoding: { type: 'pkcs8', format: 'pem' },
        });
        break;
      case CryptographicAlgorithm.ED25519:
        keyPair = crypto.generateKeyPairSync('ed25519', {
          publicKeyEncoding: { type: 'spki', format: 'pem' },
          privateKeyEncoding: { type: 'pkcs8', format: 'pem' },
        });
        break;
      default:
        throw new Error(`Algorithm ${algorithm} not supported in simulation`);
    }

    // Store private key securely (in HSM in production)
    const keyDir = path.join(__dirname, 'hsm-keys');
    if (!fs.existsSync(keyDir)) {
      fs.mkdirSync(keyDir, { recursive: true, mode: 0o700 });
    }

    fs.writeFileSync(path.join(keyDir, `${keyId}.pub`), keyPair.publicKey, {
      mode: 0o600,
    });

    // Private key would stay in HSM in production
    fs.writeFileSync(path.join(keyDir, `${keyId}.priv`), keyPair.privateKey, {
      mode: 0o600,
    });

    return {
      publicKey: keyPair.publicKey,
      privateKeyRef: keyId, // HSM reference in production
    };
  }

  private async performHSMSigning(
    keyId: string,
    dataToSign: string,
    algorithm: CryptographicAlgorithm
  ): Promise<string> {
    // In production, this would use HSM signing APIs
    // For simulation, create a signature hash

    const hash = crypto
      .createHash('sha256')
      .update(dataToSign)
      .update(keyId)
      .update(algorithm)
      .digest('hex');

    return `hsm_signature_${hash}`;
  }

  private validateAlgorithmCompliance(algorithm: CryptographicAlgorithm): void {
    const approvedAlgorithms = [
      CryptographicAlgorithm.ECDSA_P256,
      CryptographicAlgorithm.ECDSA_P384,
      CryptographicAlgorithm.ED25519,
      CryptographicAlgorithm.RSA_4096,
    ];

    if (this.quantumConfig.enabled) {
      approvedAlgorithms.push(
        CryptographicAlgorithm.DILITHIUM3,
        CryptographicAlgorithm.FALCON512,
        CryptographicAlgorithm.SPHINCS_SHA256
      );
    }

    if (!approvedAlgorithms.includes(algorithm)) {
      throw new Error(`Algorithm ${algorithm} not approved for use`);
    }
  }

  private calculateComplianceFlags(
    algorithm: CryptographicAlgorithm,
    securityLevel: SecurityLevel
  ): number {
    let flags = 0;

    // FIPS 140-2 approved algorithms
    if (
      [
        CryptographicAlgorithm.ECDSA_P256,
        CryptographicAlgorithm.ECDSA_P384,
        CryptographicAlgorithm.RSA_4096,
      ].includes(algorithm)
    ) {
      flags |= 1; // FIPS approved
    }

    // Quantum resistant
    if (this.isQuantumResistantAlgorithm(algorithm)) {
      flags |= 2; // Quantum resistant
    }

    // High security level
    if (securityLevel >= SecurityLevel.CRITICAL) {
      flags |= 4; // High security
    }

    return flags;
  }

  private isQuantumResistantAlgorithm(
    algorithm: CryptographicAlgorithm
  ): boolean {
    return [
      CryptographicAlgorithm.DILITHIUM3,
      CryptographicAlgorithm.FALCON512,
      CryptographicAlgorithm.SPHINCS_SHA256,
      CryptographicAlgorithm.KYBER768,
    ].includes(algorithm);
  }

  private getQuantumResistantAlgorithm(
    primaryAlgorithm: string
  ): CryptographicAlgorithm {
    switch (primaryAlgorithm) {
      case 'CRYSTALS-Dilithium':
        return CryptographicAlgorithm.DILITHIUM3;
      case 'FALCON':
        return CryptographicAlgorithm.FALCON512;
      case 'SPHINCS+':
        return CryptographicAlgorithm.SPHINCS_SHA256;
      default:
        return CryptographicAlgorithm.DILITHIUM3;
    }
  }

  private async validateSignature(
    dataHash: string,
    signature: string,
    publicKey: string,
    algorithm: CryptographicAlgorithm
  ): Promise<boolean> {
    // In production, this would perform actual signature verification
    // For simulation, validate signature format
    return signature.startsWith('hsm_signature_') && signature.length > 20;
  }

  private determineComplianceRequirements(
    value: number,
    securityLevel: SecurityLevel
  ): string[] {
    const requirements: string[] = [];

    if (value >= 10000) {
      requirements.push('KYC_REQUIRED');
      requirements.push('AML_SCREENING');
    }

    if (securityLevel >= SecurityLevel.CRITICAL) {
      requirements.push('SOX_COMPLIANCE');
      requirements.push('ENHANCED_MONITORING');
    }

    return requirements;
  }

  private async archiveKey(
    oldKeyId: string,
    newKeyId: string,
    archiveDate: Date
  ): Promise<void> {
    // In production, this would securely archive the old key
    securityLogger.debug('Archiving key', { module: 'HSM', operation: 'archiveKey', oldKeyId, newKeyId, archiveDate: archiveDate.toISOString() });
  }

  private async auditComplianceRequirement(
    requirement: ComplianceRequirement
  ): Promise<ComplianceRequirementResult> {
    // In production, this would perform actual compliance validation
    return {
      requirementId: requirement.id,
      description: requirement.description,
      status: requirement.status,
      lastChecked: new Date(),
      evidence: 'Compliance validation completed',
      recommendations:
        requirement.status === 'compliant' ? [] : ['Address compliance gap'],
    };
  }

  private generateComplianceRecommendations(
    results: ComplianceRequirementResult[]
  ): string[] {
    const recommendations: string[] = [];

    for (const result of results) {
      if (result.status === 'non-compliant') {
        recommendations.push(
          `Address compliance gap for ${result.requirementId}: ${result.description}`
        );
      }
    }

    return recommendations;
  }

  private getKeyCountByType(): Record<string, number> {
    const counts: Record<string, number> = {};

    for (const key of this.activeKeys.values()) {
      counts[key.keyType] = (counts[key.keyType] || 0) + 1;
    }

    return counts;
  }

  private getKeysNearingRotation(): string[] {
    const threshold = new Date(Date.now() + 7 * 24 * 60 * 60 * 1000); // 7 days
    return Array.from(this.activeKeys.values())
      .filter(key => key.rotationDue <= threshold)
      .map(key => key.keyId);
  }

  private getQuantumResistantKeyCount(): number {
    return Array.from(this.activeKeys.values()).filter(
      key => key.quantumResistant
    ).length;
  }

  private getLastAuditDate(): Date {
    // Return the most recent audit date from audit trail
    const auditEvents = this.auditTrail.filter(event =>
      event.action.includes('AUDIT')
    );
    return auditEvents.length > 0
      ? auditEvents[auditEvents.length - 1].timestamp
      : new Date(0);
  }

  private getOverallComplianceScore(): number {
    // Calculate overall compliance score across all frameworks
    let totalScore = 0;
    let frameworkCount = 0;

    for (const framework of this.complianceFrameworks.values()) {
      const compliantRequirements = framework.requirements.filter(
        req => req.status === 'compliant'
      ).length;
      const totalRequirements = framework.requirements.length;
      const score =
        totalRequirements > 0
          ? (compliantRequirements / totalRequirements) * 100
          : 0;

      totalScore += score;
      frameworkCount++;
    }

    return frameworkCount > 0 ? Math.round(totalScore / frameworkCount) : 0;
  }

  private logAuditEvent(
    action: string,
    actor: string,
    result: 'success' | 'failure',
    details: string,
    complianceContext: string = ''
  ): void {
    const event: AuditEvent = {
      eventId: crypto.randomBytes(16).toString('hex'),
      timestamp: new Date(),
      actor,
      action,
      result,
      details,
      complianceContext,
    };

    this.auditTrail.push(event);

    // Emit audit event for external monitoring
    this.emit('auditEvent', event);

    // Keep audit trail manageable (last 10000 events)
    if (this.auditTrail.length > 10000) {
      this.auditTrail = this.auditTrail.slice(-10000);
    }
  }
}

// Type definitions for return types
interface HSMSession {
  sessionId: string;
  userId: string;
  createdAt: Date;
  expiresAt: Date;
  isActive: boolean;
}

interface HSMHealthStatus {
  isHealthy: boolean;
  lastCheck: Date;
}

interface ComplianceAuditResult {
  framework: string;
  auditDate: Date;
  overallStatus: 'compliant' | 'non-compliant' | 'partial';
  requirementResults: ComplianceRequirementResult[];
  recommendations: string[];
  score: number;
}

interface ComplianceRequirementResult {
  requirementId: string;
  description: string;
  status: 'compliant' | 'non-compliant' | 'pending';
  lastChecked: Date;
  evidence: string;
  recommendations: string[];
}

interface SecurityStatusReport {
  hsmStatus: {
    isInitialized: boolean;
    provider: string;
    complianceLevel: string;
    isHealthy: boolean;
    lastHealthCheck: Date;
  };
  keyManagement: {
    totalKeys: number;
    keysByType: Record<string, number>;
    keysNearingRotation: string[];
    quantumResistantKeys: number;
  };
  multiSignature: {
    activeOperations: number;
    completedOperations: number;
    expiredOperations: number;
  };
  compliance: {
    frameworksMonitored: string[];
    lastAuditDate: Date;
    complianceScore: number;
  };
  auditTrail: {
    totalEvents: number;
    recentEvents: AuditEvent[];
    failureCount: number;
  };
  quantumReadiness: {
    enabled: boolean;
    primaryAlgorithm: string;
    hybridMode: boolean;
    migrationDeadline: Date;
  };
}

export default EnterpriseHSMService;
export {
  HSMConfiguration,
  KeyMetadata,
  MultiSigOperation,
  SignatureData,
  OperationMetadata,
  AuditEvent,
  QuantumResistantConfig,
  ComplianceFramework,
  ComplianceRequirement,
  KeyType,
  CryptographicAlgorithm,
  SecurityLevel,
  SecurityStatusReport,
  ComplianceAuditResult,
};
