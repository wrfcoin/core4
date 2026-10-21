# Decentralized Identity Management

## Overview

Wrfcoin implements a comprehensive decentralized identity management system based on W3C standards
for Decentralized Identifiers (DIDs) and Verifiable Credentials. The system enables self-sovereign
identity with privacy-preserving verification through zero-knowledge proofs.

## Decentralized Identifiers (DIDs)

### DID Method Specification

Wrfcoin uses the `did:wrf` method for decentralized identifiers:

```
did:wrf:21d39d112d4ba0491a5f463f7815a41b
```

#### Method Components

- **Method Name**: `wrf` (Wrfcoin network identifier)
- **Method-Specific ID**: 128-bit random identifier
- **Resolution**: Blockchain-based registry with universal resolver support
- **Cryptographic Curves**: Ed25519 (default) and secp256k1 support

### DID Document Structure

Standard W3C DID Document format with Wrfcoin-specific services:

```json
{
  "@context": ["https://www.w3.org/ns/did/v1", "https://w3id.org/security/suites/ed25519-2020/v1"],
  "id": "did:wrf:21d39d112d4ba0491a5f463f7815a41b",
  "controller": "did:wrf:21d39d112d4ba0491a5f463f7815a41b",
  "verificationMethod": [
    {
      "id": "did:wrf:21d39d112d4ba0491a5f463f7815a41b#key-1",
      "type": "Ed25519VerificationKey2020",
      "controller": "did:wrf:21d39d112d4ba0491a5f463f7815a41b",
      "publicKeyMultibase": "z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK"
    }
  ],
  "authentication": ["did:wrf:21d39d112d4ba0491a5f463f7815a41b#key-1"],
  "service": [
    {
      "id": "did:wrf:21d39d112d4ba0491a5f463f7815a41b#messaging",
      "type": "MessagingService",
      "serviceEndpoint": "https://messaging.wrfcoin.com/21d39d112d4ba0491a5f463f7815a41b"
    }
  ]
}
```

### System DIDs

The platform creates system DIDs for core services:

1. **Wrfcoin Issuer**: Primary credential issuer for the ecosystem
2. **Weather Station Authority**: Issues weather station licenses
3. **Compliance Auditor**: Issues compliance certificates
4. **Credential Registry**: Manages credential schemas and trust registry

## Verifiable Credentials

### Credential Schemas

The system supports three primary credential types:

#### KYC Credential Schema

```json
{
  "id": "https://wrfcoin.com/schemas/kyc/v1",
  "type": "KYCCredential",
  "properties": {
    "fullName": { "type": "string", "required": true },
    "dateOfBirth": { "type": "string", "format": "date" },
    "nationality": { "type": "string" },
    "verificationLevel": {
      "type": "string",
      "enum": ["basic", "enhanced", "premium"]
    }
  }
}
```

#### Weather Station License Schema

```json
{
  "id": "https://wrfcoin.com/schemas/weather-license/v1",
  "type": "WeatherStationLicense",
  "properties": {
    "stationId": { "type": "string", "required": true },
    "location": { "type": "object", "required": true },
    "calibrationDate": { "type": "string", "format": "date" },
    "validUntil": { "type": "string", "format": "date" }
  }
}
```

#### Compliance Certificate Schema

```json
{
  "id": "https://wrfcoin.com/schemas/compliance/v1",
  "type": "ComplianceCertificate",
  "properties": {
    "framework": { "type": "string", "required": true },
    "level": { "type": "string", "required": true },
    "issuedBy": { "type": "string", "required": true },
    "validFrom": { "type": "string", "format": "date" },
    "validUntil": { "type": "string", "format": "date" }
  }
}
```

### Credential Issuance Process

1. **Schema Validation**: Claims validated against credential schema
2. **Digital Signing**: Credential signed using issuer's private key
3. **Blockchain Anchoring**: Credential hash recorded on blockchain
4. **Holder Storage**: Credential delivered to holder's digital wallet
5. **Registry Update**: Issuance recorded in credential registry

### Credential Verification

Comprehensive verification process ensures credential integrity:

#### Verification Steps

1. **Signature Verification**: Cryptographic signature validation
2. **Issuer Resolution**: DID resolution and trust verification
3. **Expiration Check**: Validity period verification
4. **Revocation Status**: Check against revocation registry
5. **Schema Compliance**: Validation against credential schema

#### Verification Response

```json
{
  "verified": true,
  "issuer": "did:wrf:wrfcoin-issuer",
  "subject": "did:wrf:holder-did",
  "type": ["VerifiableCredential", "KYCCredential"],
  "issuanceDate": "2025-01-15T10:30:00Z",
  "verificationMethod": "Ed25519Signature2020"
}
```

## Zero-Knowledge Proof System

### Supported ZK Schemes

The system implements three zero-knowledge proof schemes:

#### BBS+ Signatures

- **Use Case**: Selective disclosure of credential attributes
- **Signature Size**: 112 bytes
- **Proof Size**: Variable based on disclosed attributes
- **Security Level**: 128-bit security

#### CL Signatures

- **Use Case**: Range proofs and predicate verification
- **Key Size**: 2048 bits
- **Proof Size**: Constant regardless of credential size
- **Predicates**: Age verification, balance proofs, membership

#### Bulletproofs

- **Use Case**: Range proofs and set membership
- **Proof Size**: Logarithmic in range size
- **Verification**: Fast verification times
- **Applications**: Financial thresholds, numeric ranges

### Proof Templates

Pre-configured proof templates for common use cases:

#### Age Verification Template

```json
{
  "schema": "prove age >= 18 without revealing exact age",
  "requiredCredentials": ["kyc_credential"],
  "predicates": ["age_over"],
  "scheme": "cl_signatures",
  "minAge": 18
}
```

#### Compliance Status Template

```json
{
  "schema": "prove compliance certification without revealing details",
  "requiredCredentials": ["compliance_certificate"],
  "predicates": ["membership_in"],
  "scheme": "bbs+",
  "allowedFrameworks": ["SOC2", "ISO27001", "PCI-DSS"]
}
```

#### Weather Station Authority Template

```json
{
  "schema": "prove weather station license without revealing location",
  "requiredCredentials": ["weather_station_license"],
  "predicates": ["selective_disclosure"],
  "scheme": "bbs+",
  "disclosedFields": ["stationId", "validUntil"]
}
```

### ZK Proof Generation

Example proof generation for age verification:

```javascript
const proof = await generateZKProof('age_verification', [kycCredential], ['age_over_18']);

// Generated proof structure
{
  "id": "proof_a1b2c3d4e5f6",
  "template": "prove age >= 18 without revealing exact age",
  "scheme": "cl_signatures",
  "proof": "base64-encoded-proof-data",
  "predicates": ["age_over_18"],
  "generatedAt": "2025-05-29T14:30:00Z",
  "validUntil": "2025-05-30T14:30:00Z",
  "credentialCount": 1
}
```

## Trust Registry and Governance

### Trust Levels

The system implements four trust levels for credential issuers:

#### Foundational Trust (Level 5)

- **Description**: Self-sovereign authorities with highest trust
- **Requirements**: Legal entity, regulatory compliance, security audit
- **Examples**: Government agencies, central banks
- **Governance**: Consortium governance model

#### Institutional Trust (Level 4)

- **Description**: Regulated entities with institutional backing
- **Requirements**: Regulatory license, compliance certificate, audit report
- **Examples**: Banks, certified authorities, accredited organizations
- **Governance**: Regulatory oversight

#### Community Trust (Level 3)

- **Description**: Peer-validated entities with community endorsement
- **Requirements**: Community vote, reputation score, verification history
- **Examples**: Industry associations, community organizations
- **Governance**: Community governance

#### Basic Trust (Level 2)

- **Description**: Verified entities with basic validation
- **Requirements**: Identity verification, basic compliance
- **Examples**: Verified businesses, certified individuals
- **Governance**: Automated validation

### Governance Framework

Decentralized governance for trust registry management:

#### Governance Operations

- **Issuer Onboarding**: Community-driven issuer approval process
- **Trust Level Changes**: Voting mechanism for trust level modifications
- **Dispute Resolution**: Decentralized arbitration for trust disputes
- **Schema Evolution**: Community governance for credential schema updates

#### Voting Mechanisms

- **Stake-Weighted Voting**: Voting power based on WRF token holdings
- **Reputation-Based Voting**: Voting weight influenced by reputation scores
- **Delegated Voting**: Ability to delegate voting rights to experts
- **Quadratic Voting**: Prevents vote concentration and encourages participation

## Credential Lifecycle Management

### Lifecycle Stages

Comprehensive management of credential lifecycle:

```yaml
Credential States:
  issued: Initial state after credential creation
  active: Credential is valid and usable
  suspended: Temporarily disabled, can be reactivated
  expired: Past validity period, can be renewed
  revoked: Permanently invalid, cannot be restored
  renewed: Updated version of expired credential
```

### Automated Lifecycle Operations

- **Expiration Monitoring**: Automated notifications for expiring credentials
- **Renewal Workflows**: Streamlined renewal process for expired credentials
- **Revocation Management**: Immediate revocation with registry updates
- **Usage Analytics**: Tracking credential verification patterns

### Notification System

Proactive communication for credential lifecycle events:

#### Notification Types

- **Expiration Warnings**: 30, 15, 7, and 1 day before expiration
- **Renewal Reminders**: Automated renewal process initiation
- **Revocation Alerts**: Immediate notification of credential revocation
- **Verification Summaries**: Regular reports on credential usage

## Privacy and Security Features

### Privacy-Preserving Technologies

- **Selective Disclosure**: Reveal only necessary credential attributes
- **Zero-Knowledge Proofs**: Prove credential possession without disclosure
- **Unlinkable Presentations**: Prevent correlation across verification events
- **Pseudonymous Interactions**: Identity privacy in credential presentations

### Security Measures

- **Cryptographic Integrity**: Digital signatures ensure credential authenticity
- **Tamper Evidence**: Any modification invalidates the credential
- **Secure Storage**: Encrypted credential storage in digital wallets
- **Access Controls**: Multi-factor authentication for credential access

### GDPR Compliance

Full compliance with European data protection regulations:

#### Privacy by Design

- **Data Minimization**: Only necessary attributes included in credentials
- **Purpose Limitation**: Credentials used only for intended purposes
- **Storage Limitation**: Automatic expiration and deletion capabilities
- **User Control**: Complete user control over credential sharing

#### Individual Rights

- **Right of Access**: Users can view all their credentials and usage
- **Right of Rectification**: Ability to update incorrect credential information
- **Right of Erasure**: Secure deletion of credentials and associated data
- **Right of Portability**: Export credentials in standard formats

## Integration and APIs

### DID Operations API

Complete API for DID lifecycle management:

```javascript
// Create a new DID
POST /api/v1/dids
{
  "keyType": "Ed25519",
  "services": ["MessagingService"]
}

// Resolve a DID
GET /api/v1/dids/{did}

// Update DID document
PUT /api/v1/dids/{did}
{
  "addServices": [...],
  "removeServices": [...],
  "rotateKeys": true
}
```

### Credential Management API

Comprehensive credential operations:

```javascript
// Issue a verifiable credential
POST /api/v1/credentials/issue
{
  "issuerDID": "did:wrf:issuer",
  "holderDID": "did:wrf:holder",
  "credentialType": "KYCCredential",
  "claims": {...}
}

// Verify a credential
POST /api/v1/credentials/verify
{
  "credential": {...},
  "checkRevocation": true
}

// Generate ZK proof
POST /api/v1/proofs/generate
{
  "template": "age_verification",
  "credentials": [...],
  "predicates": [...]
}
```

### Trust Registry API

Trust and governance operations:

```javascript
// Query trust registry
GET /api/v1/trust/issuers/{did}

// Submit governance proposal
POST /api/v1/governance/proposals
{
  "type": "add_issuer",
  "issuerDID": "did:wrf:new-issuer",
  "trustLevel": "institutional",
  "evidence": [...]
}
```

## Implementation Guidelines

### Integration Checklist

- [ ] DID resolver configuration
- [ ] Credential schema registration
- [ ] Trust registry integration
- [ ] ZK proof library setup
- [ ] Governance participation

### Best Practices

- **Key Management**: Use hardware wallets for DID private keys
- **Credential Storage**: Implement secure credential wallet storage
- **Privacy Protection**: Minimize credential disclosure in presentations
- **Verification**: Always verify credentials before accepting claims
- **Governance**: Participate in trust registry governance decisions

### Performance Considerations

- **DID Resolution**: Cache resolved DID documents for performance
- **Proof Generation**: Pre-compute proofs for frequently used templates
- **Verification**: Batch verify multiple credentials when possible
- **Storage**: Use efficient storage formats for credential collections

For technical implementation details, see the decentralized identity module at
`security/decentralized-identity-management.js`.
