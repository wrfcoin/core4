/**
 * Enterprise HSM Service - Production-Grade Implementation
 * Implements FIPS 140-2 Level 3+ compliant hardware security module integration
 * Supports quantum-resistant cryptography and enterprise compliance frameworks
 */
import { EventEmitter } from 'events';
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
declare enum KeyType {
  MASTER_SIGNING = 'master-signing',
  CROSS_CHAIN_BRIDGE = 'cross-chain-bridge',
  GOVERNANCE = 'governance',
  WEATHER_VALIDATION = 'weather-validation',
  EMERGENCY_RECOVERY = 'emergency-recovery',
  QUANTUM_RESISTANT_BACKUP = 'quantum-resistant-backup',
}
declare enum CryptographicAlgorithm {
  ECDSA_P256 = 'ECDSA-P256',
  ECDSA_P384 = 'ECDSA-P384',
  ED25519 = 'Ed25519',
  RSA_4096 = 'RSA-4096',
  DILITHIUM3 = 'CRYSTALS-Dilithium3',
  FALCON512 = 'FALCON-512',
  SPHINCS_SHA256 = 'SPHINCS+-SHA256-128s',
  KYBER768 = 'CRYSTALS-Kyber768',
}
declare enum SecurityLevel {
  STANDARD = 1,
  HIGH = 2,
  CRITICAL = 3,
  ULTRA_CRITICAL = 4,
}
declare class EnterpriseHSMService extends EventEmitter {
  private config;
  private activeKeys;
  private activeSessions;
  private multiSigOperations;
  private auditTrail;
  private complianceFrameworks;
  private quantumConfig;
  private isInitialized;
  private healthStatus;
  constructor(config: HSMConfiguration);
  /**
   * Initialize HSM connection and validate hardware security compliance
   */
  initialize(): Promise<void>;
  /**
   * Generate cryptographic key pairs in HSM with enterprise security controls
   */
  generateKeyPair(
    keyType: KeyType,
    algorithm?: CryptographicAlgorithm,
    securityLevel?: SecurityLevel
  ): Promise<KeyMetadata>;
  /**
   * Perform multi-signature operation with enterprise controls
   */
  initiateMultiSignatureOperation(
    transactionHash: string,
    requiredSignatures: number,
    threshold: number,
    operationType: string,
    value: number,
    securityLevel: SecurityLevel
  ): Promise<string>;
  /**
   * Add signature to multi-signature operation
   */
  addSignatureToOperation(
    operationId: string,
    signerId: string,
    signature: string,
    keyId: string
  ): Promise<boolean>;
  /**
   * Perform secure signing operation with HSM
   * @deprecated Use signWithHSM(keyId, dataToSign, sessionId, requestingUserId) for enhanced security
   */
  signWithHSM(
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
  signWithHSM(
    keyId: string,
    dataToSign: string,
    sessionId: string,
    requestingUserId: string
  ): Promise<{
    signature: string;
    algorithm: CryptographicAlgorithm;
    timestamp: Date;
  }>;
  /**
   * Rotate cryptographic keys with enterprise compliance
   */
  rotateKey(keyId: string, reason?: string): Promise<KeyMetadata>;
  /**
   * Enable quantum-resistant cryptography
   */
  enableQuantumResistance(
    primaryAlgorithm?: 'CRYSTALS-Dilithium' | 'FALCON' | 'SPHINCS+',
    hybridMode?: boolean
  ): Promise<void>;
  /**
   * Perform comprehensive compliance audit
   */
  performComplianceAudit(framework?: string): Promise<ComplianceAuditResult>;
  /**
   * Get comprehensive security status report
   */
  getSecurityStatusReport(): SecurityStatusReport;
  private validateInitialized;
  private validateHSMCompliance;
  private establishSecureConnection;
  private initializeKeyInfrastructure;
  private setupHealthMonitoring;
  private performHealthCheck;
  private initializeComplianceFrameworks;
  private initializeComplianceMonitoring;
  private generateSecureKeyId;
  private performHSMKeyGeneration;
  private performHSMSigning;
  private validateAlgorithmCompliance;
  private calculateComplianceFlags;
  private isQuantumResistantAlgorithm;
  private getQuantumResistantAlgorithm;
  private validateSignature;
  private determineComplianceRequirements;
  private archiveKey;
  private auditComplianceRequirement;
  private generateComplianceRecommendations;
  private getKeyCountByType;
  private getKeysNearingRotation;
  private getQuantumResistantKeyCount;
  private getLastAuditDate;
  private getOverallComplianceScore;
  private logAuditEvent;
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
//# sourceMappingURL=enterprise-hsm-service.d.ts.map
