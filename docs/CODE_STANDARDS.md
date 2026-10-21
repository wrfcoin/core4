# 📏 Code Standards & Style Guide

## Overview

This document defines the coding standards, conventions, and best practices for all WRFCoin
development.

## 🎯 General Principles

### Code Quality Principles

1. **Readability First** - Code should be easy to read and understand
2. **Consistency** - Follow established patterns and conventions
3. **Security by Design** - Consider security implications in every decision
4. **Performance Awareness** - Write efficient, scalable code
5. **Testability** - Write code that is easy to test
6. **Documentation** - Document complex logic and public APIs

### SOLID Principles

- **S**ingle Responsibility Principle
- **O**pen/Closed Principle
- **L**iskov Substitution Principle
- **I**nterface Segregation Principle
- **D**ependency Inversion Principle

## 🟡 TypeScript/JavaScript Standards

### ESLint Configuration

```typescript
// .eslintrc.js
module.exports = {
  extends: ['@typescript-eslint/recommended', 'airbnb-typescript', 'prettier'],
  rules: {
    // Enforce explicit return types
    '@typescript-eslint/explicit-function-return-type': 'error',

    // Prefer const assertions
    'prefer-const': 'error',

    // No unused variables
    '@typescript-eslint/no-unused-vars': 'error',

    // Consistent naming
    '@typescript-eslint/naming-convention': [
      'error',
      {
        selector: 'variableLike',
        format: ['camelCase', 'UPPER_CASE'],
      },
    ],
  },
};
```

### Naming Conventions

```typescript
// Variables and functions: camelCase
const weatherStationData = await fetchWeatherData();
const calculateCarbonFootprint = (data: EmissionData): number => {};

// Constants: SCREAMING_SNAKE_CASE
const MAX_RETRY_ATTEMPTS = 3;
const DEFAULT_TIMEOUT_MS = 5000;

// Types and Interfaces: PascalCase
interface WeatherStationConfig {
  id: string;
  location: GeographicCoordinate;
  sensors: SensorConfiguration[];
}

type BlockchainTransaction = {
  id: string;
  timestamp: Date;
  amount: number;
};

// Classes: PascalCase
class CarbonCreditTracker {
  private readonly emissions: EmissionRecord[];

  public calculateCredits(): CarbonCredit[] {
    // Implementation
  }
}

// Enums: PascalCase with UPPER_CASE values
enum ConsensusType {
  PROOF_OF_WORK = 'PROOF_OF_WORK',
  PROOF_OF_STAKE = 'PROOF_OF_STAKE',
  PROOF_OF_PUBLISHING = 'PROOF_OF_PUBLISHING',
}
```

### File Structure

```typescript
// 1. Imports (grouped and sorted)
import React, { useState, useEffect } from 'react';
import { Router } from 'express';

import { WeatherAPI } from '../services/weather';
import { CarbonCalculator } from '../utils/carbon';
import type { WeatherData, CarbonFootprint } from '../types';

// 2. Types and interfaces
interface ComponentProps {
  data: WeatherData;
  onUpdate: (footprint: CarbonFootprint) => void;
}

// 3. Constants
const DEFAULT_UPDATE_INTERVAL = 60000; // 1 minute

// 4. Main implementation
export const WeatherDashboard: React.FC<ComponentProps> = ({ data, onUpdate }) => {
  // Component implementation
};

// 5. Default export (if applicable)
export default WeatherDashboard;
```

### Error Handling

```typescript
// Use Result pattern for operations that can fail
type Result<T, E = Error> = { success: true; data: T } | { success: false; error: E };

// Function that returns Result
async function validateWeatherData(data: unknown): Promise<Result<WeatherData>> {
  try {
    const validated = weatherDataSchema.parse(data);
    return { success: true, data: validated };
  } catch (error) {
    return {
      success: false,
      error: new ValidationError('Invalid weather data', error),
    };
  }
}

// Usage
const result = await validateWeatherData(rawData);
if (result.success) {
  // Use result.data safely
  processWeatherData(result.data);
} else {
  // Handle error
  logger.error('Weather data validation failed', result.error);
}
```

### Async/Await Patterns

```typescript
// Prefer async/await over Promises
// Good
async function fetchAndProcessData(): Promise<ProcessedData> {
  try {
    const rawData = await fetchWeatherData();
    const validated = await validateData(rawData);
    return await processData(validated);
  } catch (error) {
    logger.error('Data processing failed', error);
    throw new ProcessingError('Failed to process weather data', error);
  }
}

// Avoid: Promise chains
function fetchAndProcessDataBad(): Promise<ProcessedData> {
  return fetchWeatherData()
    .then(validateData)
    .then(processData)
    .catch(error => {
      logger.error('Data processing failed', error);
      throw error;
    });
}
```

### Documentation Standards

````typescript
/**
 * Calculates carbon credits based on verified emission reductions.
 *
 * @param emissions - Array of emission records to process
 * @param baseline - Baseline emission level for comparison
 * @param methodology - Calculation methodology to use
 * @returns Promise resolving to calculated carbon credits
 *
 * @throws {ValidationError} When emission data is invalid
 * @throws {CalculationError} When calculation fails
 *
 * @example
 * ```typescript
 * const credits = await calculateCarbonCredits(
 *   emissions,
 *   baselineEmissions,
 *   'verified-carbon-standard'
 * );
 * ```
 */
async function calculateCarbonCredits(
  emissions: EmissionRecord[],
  baseline: number,
  methodology: string
): Promise<CarbonCredit[]> {
  // Implementation
}
````

## 🦀 Rust Standards

### Code Organization

```rust
// src/lib.rs or main.rs
//! WRFCoin Blockchain Core
//!
//! This crate provides the core blockchain functionality for WRFCoin,
//! including consensus mechanisms, transaction processing, and climate
//! data integration.

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use thiserror::Error;

// Re-exports
pub use crate::consensus::ConsensusEngine;
pub use crate::transaction::Transaction;

// Modules
pub mod consensus;
pub mod crypto;
pub mod storage;
pub mod transaction;

// Types
pub type BlockHash = [u8; 32];
pub type Address = [u8; 20];
```

### Naming Conventions

```rust
// Modules: snake_case
mod weather_data;
mod carbon_credits;

// Types: PascalCase
struct WeatherStation {
    id: String,
    location: GeographicCoordinate,
    sensors: Vec<Sensor>,
}

// Functions and variables: snake_case
fn calculate_carbon_footprint(emissions: &[Emission]) -> Result<f64, CalculationError> {
    let total_emissions = emissions.iter().sum();
    Ok(total_emissions * CARBON_FACTOR)
}

// Constants: SCREAMING_SNAKE_CASE
const MAX_BLOCK_SIZE: usize = 1_048_576; // 1MB
const CONSENSUS_TIMEOUT: Duration = Duration::from_secs(30);

// Traits: PascalCase with descriptive names
trait ConsensusValidator {
    fn validate_block(&self, block: &Block) -> Result<(), ValidationError>;
}
```

### Error Handling

```rust
use thiserror::Error;

// Define custom error types
#[derive(Error, Debug)]
pub enum BlockchainError {
    #[error("Invalid transaction: {message}")]
    InvalidTransaction { message: String },

    #[error("Consensus failure: {0}")]
    ConsensusFailed(#[from] ConsensusError),

    #[error("Storage error: {0}")]
    Storage(#[from] StorageError),

    #[error("Cryptographic error: {0}")]
    Crypto(#[from] CryptoError),
}

// Use Result for fallible operations
fn validate_transaction(tx: &Transaction) -> Result<(), BlockchainError> {
    if tx.amount <= 0 {
        return Err(BlockchainError::InvalidTransaction {
            message: "Transaction amount must be positive".to_string(),
        });
    }

    // Additional validation...
    Ok(())
}
```

### Documentation

````rust
/// Represents a block in the WRFCoin blockchain.
///
/// Each block contains a header with metadata and a body with transactions.
/// Blocks are validated using the configured consensus mechanism.
///
/// # Examples
///
/// ```
/// use wrfcoin_core::{Block, Transaction};
///
/// let block = Block::new(
///     previous_hash,
///     vec![transaction1, transaction2],
///     timestamp,
/// );
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    /// Block header containing metadata
    pub header: BlockHeader,
    /// Transactions included in this block
    pub transactions: Vec<Transaction>,
}

impl Block {
    /// Creates a new block with the given parameters.
    ///
    /// # Arguments
    ///
    /// * `previous_hash` - Hash of the previous block
    /// * `transactions` - List of transactions to include
    /// * `timestamp` - Block creation timestamp
    ///
    /// # Returns
    ///
    /// A new `Block` instance ready for validation.
    pub fn new(
        previous_hash: BlockHash,
        transactions: Vec<Transaction>,
        timestamp: u64,
    ) -> Self {
        // Implementation
    }
}
````

### Performance Patterns

```rust
// Use appropriate data structures
use std::collections::HashMap;
use indexmap::IndexMap; // For ordered maps
use dashmap::DashMap; // For concurrent access

// Prefer borrowing over cloning
fn process_transactions(txs: &[Transaction]) -> Vec<ProcessedTransaction> {
    txs.iter()
       .filter(|tx| tx.is_valid())
       .map(|tx| process_transaction(tx))
       .collect()
}

// Use Cow for conditional cloning
use std::borrow::Cow;

fn normalize_address(addr: &str) -> Cow<str> {
    if addr.starts_with("0x") {
        Cow::Borrowed(addr)
    } else {
        Cow::Owned(format!("0x{}", addr))
    }
}
```

## 🔷 Solidity Standards

### Contract Structure

```solidity
// SPDX-License-Identifier: MIT
pragma solidity ^0.8.19;

import "@openzeppelin/contracts/access/Ownable.sol";
import "@openzeppelin/contracts/security/ReentrancyGuard.sol";

/**
 * @title CarbonCreditTracker
 * @dev Tracks and manages carbon credits on the WRFCoin platform
 * @author WRFCoin Team
 */
contract CarbonCreditTracker is Ownable, ReentrancyGuard {

    // State variables
    mapping(address => uint256) private _carbonCredits;
    mapping(bytes32 => CarbonProject) private _projects;

    uint256 private _totalSupply;
    uint256 private constant MAX_SUPPLY = 1000000 * 10**18;

    // Events
    event CarbonCreditsIssued(
        address indexed recipient,
        uint256 amount,
        bytes32 indexed projectId
    );

    // Modifiers
    modifier onlyValidProject(bytes32 projectId) {
        require(_projects[projectId].isValid, "Invalid project");
        _;
    }

    // Functions grouped by visibility
    // External functions
    // Public functions
    // Internal functions
    // Private functions
}
```

### Naming Conventions

```solidity
// Contracts: PascalCase
contract CarbonCreditMarketplace { }

// Functions: camelCase
function calculateCarbonFootprint(uint256 emissions)
    external
    pure
    returns (uint256)
{
    // Implementation
}

// State variables: camelCase with underscore prefix for private
uint256 private _totalSupply;
mapping(address => uint256) public carbonCredits;

// Constants: UPPER_CASE
uint256 public constant MAX_CREDITS_PER_PROJECT = 100000;
bytes32 private constant DOMAIN_SEPARATOR = keccak256("WRFCoin");

// Events: PascalCase
event CarbonCreditsTransferred(
    address indexed from,
    address indexed to,
    uint256 amount
);

// Modifiers: camelCase
modifier onlyAuthorizedValidator() {
    require(isValidator[msg.sender], "Not authorized");
    _;
}
```

### Security Patterns

```solidity
// Use checks-effects-interactions pattern
function withdrawFunds(uint256 amount) external nonReentrant {
    // Checks
    require(amount > 0, "Amount must be positive");
    require(balances[msg.sender] >= amount, "Insufficient balance");

    // Effects
    balances[msg.sender] -= amount;

    // Interactions
    (bool success, ) = msg.sender.call{value: amount}("");
    require(success, "Transfer failed");
}

// Use SafeMath for older Solidity versions (< 0.8.0)
// For 0.8.0+, arithmetic operations have built-in overflow protection

// Input validation
function setProjectDetails(
    bytes32 projectId,
    string calldata name,
    uint256 expectedCredits
) external onlyOwner {
    require(projectId != bytes32(0), "Invalid project ID");
    require(bytes(name).length > 0, "Name cannot be empty");
    require(expectedCredits > 0, "Expected credits must be positive");

    // Implementation
}
```

### Gas Optimization

```solidity
// Pack structs efficiently
struct CarbonProject {
    bool isValid;        // 1 byte
    uint8 projectType;   // 1 byte
    uint16 year;         // 2 bytes
    uint32 timestamp;    // 4 bytes
    // Total: 8 bytes in one slot

    address owner;       // 20 bytes - new slot
    uint256 totalCredits; // 32 bytes - new slot
}

// Use events for data that doesn't need on-chain storage
event ProjectDataUpdated(
    bytes32 indexed projectId,
    string metadata, // Store in event log, not state
    uint256 timestamp
);

// Prefer mappings over arrays for lookups
mapping(address => bool) public authorizedValidators;

// Use unchecked for trusted arithmetic (Solidity 0.8.0+)
function calculateReward(uint256 baseAmount) internal pure returns (uint256) {
    unchecked {
        return baseAmount * 110 / 100; // Safe: no overflow possible
    }
}
```

## 🗄️ Database Standards

### Schema Design

```sql
-- Use descriptive table names
CREATE TABLE weather_stations (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name VARCHAR(255) NOT NULL,
    location POINT NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),

    -- Indexes
    INDEX idx_weather_stations_location USING GIST (location),
    INDEX idx_weather_stations_created_at (created_at)
);

-- Use foreign keys for referential integrity
CREATE TABLE sensor_readings (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    weather_station_id UUID NOT NULL REFERENCES weather_stations(id),
    sensor_type VARCHAR(50) NOT NULL,
    value DECIMAL(10,4) NOT NULL,
    unit VARCHAR(20) NOT NULL,
    recorded_at TIMESTAMP WITH TIME ZONE NOT NULL,

    INDEX idx_sensor_readings_station_type (weather_station_id, sensor_type),
    INDEX idx_sensor_readings_recorded_at (recorded_at)
);
```

### TypeScript Schema (Drizzle)

```typescript
import { pgTable, uuid, varchar, timestamp, decimal, point } from 'drizzle-orm/pg-core';

export const weatherStations = pgTable('weather_stations', {
  id: uuid('id').primaryKey().defaultRandom(),
  name: varchar('name', { length: 255 }).notNull(),
  location: point('location').notNull(),
  createdAt: timestamp('created_at', { withTimezone: true }).defaultNow(),
  updatedAt: timestamp('updated_at', { withTimezone: true }).defaultNow(),
});

export const sensorReadings = pgTable('sensor_readings', {
  id: uuid('id').primaryKey().defaultRandom(),
  weatherStationId: uuid('weather_station_id')
    .references(() => weatherStations.id)
    .notNull(),
  sensorType: varchar('sensor_type', { length: 50 }).notNull(),
  value: decimal('value', { precision: 10, scale: 4 }).notNull(),
  unit: varchar('unit', { length: 20 }).notNull(),
  recordedAt: timestamp('recorded_at', { withTimezone: true }).notNull(),
});
```

## 🧪 Testing Standards

### Unit Test Structure

```typescript
// test/services/carbonCalculator.test.ts
import { describe, test, expect, beforeEach, vi } from 'vitest';
import { CarbonCalculator } from '../src/services/CarbonCalculator';
import type { EmissionData } from '../src/types';

describe('CarbonCalculator', () => {
  let calculator: CarbonCalculator;

  beforeEach(() => {
    calculator = new CarbonCalculator();
  });

  describe('calculateFootprint', () => {
    test('should calculate carbon footprint correctly for valid data', async () => {
      // Arrange
      const emissionData: EmissionData = {
        electricity: 1000, // kWh
        transportation: 500, // km
        heating: 200, // therms
      };

      // Act
      const result = await calculator.calculateFootprint(emissionData);

      // Assert
      expect(result.totalEmissions).toBeCloseTo(2.45, 2); // tons CO2
      expect(result.breakdown).toBeDefined();
      expect(result.breakdown.electricity).toBeCloseTo(0.85, 2);
    });

    test('should handle zero emissions correctly', async () => {
      // Arrange
      const emissionData: EmissionData = {
        electricity: 0,
        transportation: 0,
        heating: 0,
      };

      // Act
      const result = await calculator.calculateFootprint(emissionData);

      // Assert
      expect(result.totalEmissions).toBe(0);
    });

    test('should throw error for negative values', async () => {
      // Arrange
      const emissionData: EmissionData = {
        electricity: -100,
        transportation: 500,
        heating: 200,
      };

      // Act & Assert
      await expect(calculator.calculateFootprint(emissionData)).rejects.toThrow(
        'Emission values cannot be negative'
      );
    });
  });
});
```

### Integration Test Example

```typescript
// test/integration/weatherApi.integration.test.ts
import { describe, test, expect, beforeAll, afterAll } from 'vitest';
import { testDb, setupTestDatabase, teardownTestDatabase } from '../helpers/database';
import { WeatherAPI } from '../src/api/WeatherAPI';
import { WeatherStationService } from '../src/services/WeatherStationService';

describe('Weather API Integration', () => {
  let weatherAPI: WeatherAPI;
  let weatherService: WeatherStationService;

  beforeAll(async () => {
    await setupTestDatabase();
    weatherService = new WeatherStationService(testDb);
    weatherAPI = new WeatherAPI(weatherService);
  });

  afterAll(async () => {
    await teardownTestDatabase();
  });

  test('should create weather station and retrieve data', async () => {
    // Create weather station
    const station = await weatherAPI.createStation({
      name: 'Test Station',
      location: { lat: 40.7128, lon: -74.006 },
    });

    expect(station.id).toBeDefined();
    expect(station.name).toBe('Test Station');

    // Add sensor reading
    const reading = await weatherAPI.addReading(station.id, {
      sensorType: 'temperature',
      value: 25.5,
      unit: 'celsius',
    });

    expect(reading.value).toBe(25.5);

    // Retrieve readings
    const readings = await weatherAPI.getReadings(station.id);
    expect(readings).toHaveLength(1);
    expect(readings[0].value).toBe(25.5);
  });
});
```

## 📁 File Organization

### Project Structure

```
wrfcoin/
├── frontend/                 # React TypeScript application
│   ├── src/
│   │   ├── components/       # Reusable UI components
│   │   ├── pages/           # Page components
│   │   ├── hooks/           # Custom React hooks
│   │   ├── services/        # API service layer
│   │   ├── utils/           # Utility functions
│   │   ├── types/           # TypeScript type definitions
│   │   └── styles/          # CSS/styling files
│   ├── public/              # Static assets
│   └── tests/               # Frontend tests
│
├── backend/                 # Node.js TypeScript services
│   ├── src/
│   │   ├── api/             # API route handlers
│   │   ├── services/        # Business logic layer
│   │   ├── middleware/      # Express middleware
│   │   ├── database/        # Database configuration
│   │   ├── utils/           # Utility functions
│   │   └── types/           # TypeScript type definitions
│   └── tests/               # Backend tests
│
├── blockchain-core/         # Rust blockchain implementation
│   ├── src/
│   │   ├── consensus/       # Consensus mechanisms
│   │   ├── crypto/          # Cryptographic functions
│   │   ├── storage/         # Storage layer
│   │   ├── network/         # P2P networking
│   │   └── lib.rs           # Main library file
│   └── tests/               # Rust tests
│
├── contracts/               # Solidity smart contracts
│   ├── src/                 # Contract source files
│   ├── tests/               # Contract tests
│   └── scripts/             # Deployment scripts
│
├── docs/                    # Documentation
│   ├── api/                 # API documentation
│   ├── architecture/        # Architecture docs
│   └── guides/              # User guides
│
├── scripts/                 # Development and deployment scripts
└── tests/                   # Cross-cutting integration tests
```

### Import Organization

```typescript
// 1. Node modules (alphabetical)
import express from 'express';
import { z } from 'zod';

// 2. Internal modules (relative paths, alphabetical)
import { DatabaseService } from '../database/DatabaseService';
import { WeatherAPI } from '../api/WeatherAPI';
import { logger } from '../utils/logger';

// 3. Type imports (separate from value imports)
import type { WeatherData, StationConfig } from '../types/weather';
import type { Request, Response } from 'express';
```

## 🔧 Development Tools

### VS Code Configuration

```json
// .vscode/settings.json
{
  "typescript.preferences.includePackageJsonAutoImports": "off",
  "editor.formatOnSave": true,
  "editor.codeActionsOnSave": {
    "source.fixAll.eslint": true,
    "source.organizeImports": true
  },
  "rust-analyzer.checkOnSave.command": "clippy",
  "solidity.formatter": "prettier",
  "[typescript]": {
    "editor.defaultFormatter": "esbenp.prettier-vscode"
  },
  "[rust]": {
    "editor.defaultFormatter": "rust-lang.rust-analyzer"
  }
}
```

### Pre-commit Hooks

```bash
#!/bin/sh
# .husky/pre-commit

# Run type checking
npm run type-check

# Run linting
npm run lint

# Run tests
npm run test:unit

# Run Rust checks
cd blockchain-core && cargo clippy -- -D warnings
cd blockchain-core && cargo fmt --check

# Run smart contract checks
npm run compile
```

## 📊 Quality Metrics

### Code Quality Goals

- **Test Coverage**: > 80% line coverage
- **Type Coverage**: > 95% (TypeScript)
- **Linting**: Zero violations
- **Security**: Zero high/critical vulnerabilities
- **Performance**: < 200ms API response time
- **Documentation**: All public APIs documented

### Monitoring

```bash
# Generate quality reports
npm run quality:report
npm run coverage:check
npm run security:audit

# View quality dashboard
npm run quality:dashboard
```

---

These standards ensure **consistency**, **quality**, and **maintainability** across the entire
WRFCoin platform while supporting **scalable development** and **enterprise-grade** requirements.
