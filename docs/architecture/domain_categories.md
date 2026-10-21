# WRFCoin Codebase Domain Categories

## Overview

This document defines the domain categories for the WRFCoin blockchain platform codebase inventory
and mapping system. These categories organize the 15,570+ files across the comprehensive
climate-focused blockchain ecosystem.

## Domain Category Definitions

### 1. **blockchain-core**

**Description**: Core blockchain implementation in Rust

- **Technologies**: Rust, Cargo
- **Key Components**: Consensus mechanisms, cryptography, P2P networking, mining, storage
- **File Patterns**:
  - `blockchain-core/**/*`
  - `*.rs` files in core directories
  - `Cargo.toml`, `Cargo.lock` in blockchain context
- **Examples**: `blockchain-core/consensus/`, `blockchain-core/crypto/`, `blockchain-core/lib.rs`

### 2. **backend-services**

**Description**: Node.js microservices and API infrastructure

- **Technologies**: Node.js, TypeScript, Express
- **Key Components**: 15+ microservices, API gateways, database services
- **File Patterns**:
  - `backend/**/*`
  - `backend/server/**/*`
  - `backend/microservices/**/*`
  - API and service-related `.js`, `.ts` files
- **Examples**: `backend/server/enhanced_unified_api.ts`, `backend/routes/`, `backend/services/`

### 3. **frontend-interface**

**Description**: React/TypeScript web dashboard and user interface

- **Technologies**: React, TypeScript, Vite, Material-UI
- **Key Components**: Dashboard, governance interface, wallet operations
- **File Patterns**:
  - `frontend/**/*`
  - React components (`.tsx`, `.jsx`)
  - Frontend styles (`.css`, `.scss`)
- **Examples**: `frontend/src/components/`, `frontend/src/pages/`, `frontend/src/hooks/`

### 4. **smart-contracts**

**Description**: Solidity smart contracts and EVM integration

- **Technologies**: Solidity, Hardhat, OpenZeppelin
- **Key Components**: Token contracts, governance, DeFi protocols
- **File Patterns**:
  - `contracts/**/*`
  - `smart-contracts/**/*`
  - `*.sol` files
  - Contract deployment and test files
- **Examples**: `contracts/WRFToken.sol`, `contracts/CarbonCredits.sol`,
  `contracts/ComputationalNFT.sol`

### 5. **infrastructure-devops**

**Description**: DevOps, deployment, and infrastructure management

- **Technologies**: Docker, Kubernetes, Terraform
- **Key Components**: Container orchestration, monitoring, deployment scripts
- **File Patterns**:
  - `infrastructure/**/*`
  - `docker-compose*.yml`
  - `Dockerfile*`
  - Kubernetes manifests (`*.yaml`, `*.yml`)
  - Deployment scripts
- **Examples**: `infrastructure/kubernetes/`, `docker-compose.yml`, `infrastructure/terraform/`

### 6. **automation-scripts**

**Description**: Automation scripts, utilities, and workflow management

- **Technologies**: Python, Bash, Node.js
- **Key Components**: Collection scripts, deployment automation, utilities
- **File Patterns**:
  - `scripts/**/*`
  - Executable scripts (`.py`, `.js`, `.sh`)
  - Workflow and automation files
- **Examples**: `scripts/collectors/`, `scripts/workflow-dependency-manager.js`

### 7. **testing-quality**

**Description**: Testing frameworks, quality assurance, and validation

- **Technologies**: Jest, Playwright, Artillery, Pytest
- **Key Components**: Unit tests, integration tests, E2E tests, performance tests
- **File Patterns**:
  - `test/**/*`, `tests/**/*`
  - `consolidated-tests/**/*`
  - `*test*.js`, `*test*.ts`, `*test*.py`
  - Test configuration files
- **Examples**: `consolidated-tests/`, `test/`, `tests/`, `*.test.js`

### 8. **documentation**

**Description**: Documentation, guides, and architectural documentation

- **Technologies**: Markdown, Sphinx, MkDocs
- **Key Components**: API docs, architecture guides, user documentation
- **File Patterns**:
  - `docs/**/*`
  - `*.md` files
  - `README*` files
  - Documentation assets
- **Examples**: `docs/`, `README.md`, `CLAUDE.md`, `PROJECT_STRUCTURE.md`

### 9. **security-compliance**

**Description**: Security implementations, compliance frameworks, and auditing

- **Technologies**: HSM integration, compliance tools
- **Key Components**: Security hardening, ISO compliance, audit frameworks
- **File Patterns**:
  - `security/**/*`
  - Security-related configuration files
  - Compliance and audit files
- **Examples**: `security/hsm-secure-implementation.js`, `blockchain-core/iso_compliance/`

### 10. **configuration-data**

**Description**: Configuration files, data assets, and static resources

- **Technologies**: JSON, YAML, TOML, ENV
- **Key Components**: App configuration, environment variables, static assets
- **File Patterns**:
  - `config/**/*`
  - `*.json`, `*.yaml`, `*.yml`, `*.toml`, `*.env`
  - `assets/**/*`, `static/**/*`
  - Database schema and migration files
- **Examples**: `config/`, `package.json`, `*.env`, `assets/`, `*.sql`

## Multi-Domain Files

Some files may belong to multiple domains. The categorization script handles these cases by:

1. **Primary Domain**: The most specific/relevant domain
2. **Secondary Domains**: Additional applicable domains
3. **Context-Based**: Determined by file location and content analysis

### Examples of Multi-Domain Files:

- `docker-compose.yml` → Primary: **infrastructure-devops**, Secondary: **configuration-data**
- `backend/shared/schema.ts` → Primary: **backend-services**, Secondary: **configuration-data**
- `scripts/test-*.js` → Primary: **testing-quality**, Secondary: **automation-scripts**

## File Count Distribution (Estimated)

Based on the manifest analysis:

| Domain                    | Estimated Files | Key Extensions                   |
| ------------------------- | --------------- | -------------------------------- |
| **configuration-data**    | ~4,200          | `.sql`, `.json`, `.yaml`, `.env` |
| **automation-scripts**    | ~2,800          | `.py`, `.js`, `.sh`              |
| **documentation**         | ~1,500          | `.md`, `.txt`                    |
| **backend-services**      | ~1,800          | `.js`, `.ts` (backend context)   |
| **blockchain-core**       | ~800            | `.rs`, `Cargo.*`                 |
| **frontend-interface**    | ~600            | `.jsx`, `.tsx`, `.css`           |
| **testing-quality**       | ~400            | `*test*`, test configs           |
| **infrastructure-devops** | ~200            | `docker*`, k8s manifests         |
| **smart-contracts**       | ~100            | `.sol`                           |
| **security-compliance**   | ~80             | Security-specific files          |

## Pattern Matching Rules

The categorization system uses these pattern matching approaches:

### 1. **Path-Based Matching**

- Exact directory matches (e.g., `frontend/`, `backend/`)
- Subdirectory patterns (e.g., `*/test/*`, `*/docs/*`)

### 2. **Extension-Based Matching**

- Technology-specific extensions (`.rs` → blockchain-core)
- Context-dependent extensions (`.js` → varies by location)

### 3. **Filename Pattern Matching**

- Naming conventions (`*test*`, `*config*`, `docker*`)
- Special files (`README*`, `CLAUDE.md`, `package.json`)

### 4. **Content-Based Analysis**

- File content patterns for ambiguous cases
- Import/dependency analysis for relationships

## Validation and Quality Assurance

### Categorization Validation:

1. **Coverage Verification**: All 15,570 files must be categorized
2. **Pattern Testing**: Validate pattern rules against sample files
3. **Multi-Domain Handling**: Ensure proper primary/secondary assignment
4. **Edge Case Testing**: Handle unusual file types and locations
5. **Manual Review**: Sample validation for categorization accuracy

### Maintenance:

- Regular pattern rule updates for new file types
- Validation against file manifest changes
- Integration with CI/CD for automated checking

---

## Usage

This taxonomy is used by:

1. **File Domain Mapping Script**: Automated categorization
2. **Responsibility Matrix**: Maintainer assignment by domain
3. **Dependency Analysis**: Cross-domain relationship mapping
4. **Visualization Tools**: Domain-based architectural diagrams
5. **CI/CD Integration**: Automated validation and updates

---

**Last Updated**: July 18, 2025  
**Version**: 1.0.0  
**Total Files Covered**: 15,570  
**Validation Status**: ✅ Comprehensive coverage defined
