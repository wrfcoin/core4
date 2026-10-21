/**
 * API Schema Contract Tests
 *
 * Validates request/response schemas for WRFCoin microservice APIs.
 * These tests are lightweight and do not require running services -
 * they validate the data contracts (shapes, types, constraints) that
 * consumers and providers agree on.
 *
 * For full Pact-based consumer-driven contract tests, see:
 * - api-blockchain.contract.test.js
 * - api-weather.contract.test.js
 * - api-postgres.contract.test.js
 *
 * Run: npx jest tests/contract/api-schema-contracts.test.ts
 */

import { describe, it, expect } from '@jest/globals';

// ---- Schema Validators ----

const ETHEREUM_ADDRESS = /^0x[0-9a-fA-F]{40}$/;
const TX_HASH = /^0x[0-9a-fA-F]{64}$/;
const ISO_DATETIME = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}/;
const UUID_V4 = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;

type SchemaField = {
  type: 'string' | 'number' | 'boolean' | 'object' | 'array';
  required?: boolean;
  pattern?: RegExp;
  min?: number;
  max?: number;
  items?: Schema;
  properties?: Schema;
  enum?: string[];
  nullable?: boolean;
};

type Schema = Record<string, SchemaField>;

function validateSchema(data: Record<string, unknown>, schema: Schema): string[] {
  const errors: string[] = [];

  for (const [field, def] of Object.entries(schema)) {
    const value = data[field];

    if (value === undefined || value === null) {
      if (def.required && !def.nullable) {
        errors.push(`Missing required field: ${field}`);
      }
      continue;
    }

    if (def.type === 'array') {
      if (!Array.isArray(value)) {
        errors.push(`${field}: expected array, got ${typeof value}`);
      } else if (def.items && value.length > 0) {
        const itemErrors = validateSchema(value[0] as Record<string, unknown>, def.items);
        errors.push(...itemErrors.map((e) => `${field}[0].${e}`));
      }
      continue;
    }

    if (def.type === 'object') {
      if (typeof value !== 'object') {
        errors.push(`${field}: expected object, got ${typeof value}`);
      } else if (def.properties) {
        const objErrors = validateSchema(value as Record<string, unknown>, def.properties);
        errors.push(...objErrors.map((e) => `${field}.${e}`));
      }
      continue;
    }

    if (typeof value !== def.type) {
      errors.push(`${field}: expected ${def.type}, got ${typeof value}`);
      continue;
    }

    if (def.pattern && typeof value === 'string' && !def.pattern.test(value)) {
      errors.push(`${field}: value "${value}" doesn't match pattern ${def.pattern}`);
    }

    if (def.min !== undefined && typeof value === 'number' && value < def.min) {
      errors.push(`${field}: value ${value} below minimum ${def.min}`);
    }

    if (def.max !== undefined && typeof value === 'number' && value > def.max) {
      errors.push(`${field}: value ${value} above maximum ${def.max}`);
    }

    if (def.enum && typeof value === 'string' && !def.enum.includes(value)) {
      errors.push(`${field}: value "${value}" not in enum [${def.enum.join(', ')}]`);
    }
  }

  return errors;
}

// ---- API Response Envelope ----

describe('API Schema Contracts: Response Envelope', () => {
  const RESPONSE_SCHEMA: Schema = {
    success: { type: 'boolean', required: true },
    data: { type: 'object', required: false },
    error: { type: 'string', required: false },
    timestamp: { type: 'string', required: true, pattern: ISO_DATETIME },
  };

  it('should validate success response envelope', () => {
    const response = {
      success: true,
      data: { id: 1 },
      timestamp: new Date().toISOString(),
    };
    expect(validateSchema(response, RESPONSE_SCHEMA)).toEqual([]);
  });

  it('should validate error response envelope', () => {
    const response = {
      success: false,
      error: 'Resource not found',
      timestamp: new Date().toISOString(),
    };
    expect(validateSchema(response, RESPONSE_SCHEMA)).toEqual([]);
  });

  it('should reject envelope missing success field', () => {
    const response = {
      data: { id: 1 },
      timestamp: new Date().toISOString(),
    };
    const errors = validateSchema(response, RESPONSE_SCHEMA);
    expect(errors).toContain('Missing required field: success');
  });

  it('should reject envelope missing timestamp', () => {
    const response = {
      success: true,
      data: {},
    };
    const errors = validateSchema(response, RESPONSE_SCHEMA);
    expect(errors).toContain('Missing required field: timestamp');
  });
});

// ---- Blockchain API Contracts ----

describe('API Schema Contracts: Blockchain', () => {
  const BLOCK_SCHEMA: Schema = {
    hash: { type: 'string', required: true, pattern: TX_HASH },
    height: { type: 'number', required: true, min: 0 },
    timestamp: { type: 'string', required: true, pattern: ISO_DATETIME },
    prevHash: { type: 'string', required: true, pattern: TX_HASH },
    transactionCount: { type: 'number', required: true, min: 0 },
    difficulty: { type: 'number', required: true, min: 0 },
    size: { type: 'number', required: true, min: 0 },
    merkleRoot: { type: 'string', required: true },
  };

  const TRANSACTION_SCHEMA: Schema = {
    txId: { type: 'string', required: true, pattern: TX_HASH },
    fromAddress: { type: 'string', required: true, pattern: ETHEREUM_ADDRESS },
    toAddress: { type: 'string', required: true, pattern: ETHEREUM_ADDRESS },
    amount: { type: 'number', required: true, min: 0 },
    fee: { type: 'number', required: true, min: 0 },
    timestamp: { type: 'string', required: true, pattern: ISO_DATETIME },
    status: { type: 'string', required: true, enum: ['pending', 'confirmed', 'failed'] },
    type: { type: 'string', required: true },
  };

  it('should validate block response schema', () => {
    const block = {
      hash: '0x' + 'a'.repeat(64),
      height: 12345,
      timestamp: '2025-01-15T10:30:00Z',
      prevHash: '0x' + 'b'.repeat(64),
      transactionCount: 5,
      difficulty: 1,
      size: 1024,
      merkleRoot: '0x' + 'c'.repeat(64),
    };
    expect(validateSchema(block, BLOCK_SCHEMA)).toEqual([]);
  });

  it('should reject block with negative height', () => {
    const block = {
      hash: '0x' + 'a'.repeat(64),
      height: -1,
      timestamp: '2025-01-15T10:30:00Z',
      prevHash: '0x' + 'b'.repeat(64),
      transactionCount: 0,
      difficulty: 1,
      size: 256,
      merkleRoot: '0x' + 'c'.repeat(64),
    };
    const errors = validateSchema(block, BLOCK_SCHEMA);
    expect(errors.some((e) => e.includes('height'))).toBe(true);
  });

  it('should validate transaction response schema', () => {
    const tx = {
      txId: '0x' + 'a'.repeat(64),
      fromAddress: '0x' + '1'.repeat(40),
      toAddress: '0x' + '2'.repeat(40),
      amount: 100.5,
      fee: 0.01,
      timestamp: '2025-01-15T10:30:00Z',
      status: 'confirmed',
      type: 'transfer',
    };
    expect(validateSchema(tx, TRANSACTION_SCHEMA)).toEqual([]);
  });

  it('should reject transaction with invalid status', () => {
    const tx = {
      txId: '0x' + 'a'.repeat(64),
      fromAddress: '0x' + '1'.repeat(40),
      toAddress: '0x' + '2'.repeat(40),
      amount: 100,
      fee: 0.01,
      timestamp: '2025-01-15T10:30:00Z',
      status: 'unknown',
      type: 'transfer',
    };
    const errors = validateSchema(tx, TRANSACTION_SCHEMA);
    expect(errors.some((e) => e.includes('status'))).toBe(true);
  });

  it('should reject transaction with invalid address format', () => {
    const tx = {
      txId: '0x' + 'a'.repeat(64),
      fromAddress: 'not-an-address',
      toAddress: '0x' + '2'.repeat(40),
      amount: 100,
      fee: 0.01,
      timestamp: '2025-01-15T10:30:00Z',
      status: 'pending',
      type: 'transfer',
    };
    const errors = validateSchema(tx, TRANSACTION_SCHEMA);
    expect(errors.some((e) => e.includes('fromAddress'))).toBe(true);
  });
});

// ---- Weather API Contracts ----

describe('API Schema Contracts: Weather Data', () => {
  const WEATHER_STATION_SCHEMA: Schema = {
    id: { type: 'string', required: true },
    stationId: { type: 'string', required: true },
    location: { type: 'string', required: true },
    isActive: { type: 'boolean', required: true },
    qualityScore: { type: 'number', required: true, min: 0, max: 100 },
    totalRewards: { type: 'string', required: true },
  };

  const WEATHER_DATA_SCHEMA: Schema = {
    stationId: { type: 'string', required: true },
    timestamp: { type: 'string', required: true, pattern: ISO_DATETIME },
    temperature: { type: 'number', required: true, min: -8920, max: 7070 },
    humidity: { type: 'number', required: true, min: 0, max: 10000 },
    pressure: { type: 'number', required: true, min: 87000, max: 108380 },
    windSpeed: { type: 'number', required: true, min: 0, max: 11300 },
    confidence: { type: 'number', required: true, min: 0, max: 100 },
  };

  it('should validate weather station schema', () => {
    const station = {
      id: 'station-001',
      stationId: 'WS-NYC-001',
      location: '40.7128,-74.006',
      isActive: true,
      qualityScore: 85,
      totalRewards: '1000000000000000000',
    };
    expect(validateSchema(station, WEATHER_STATION_SCHEMA)).toEqual([]);
  });

  it('should reject station with quality score > 100', () => {
    const station = {
      id: 'station-001',
      stationId: 'WS-NYC-001',
      location: '40.7128,-74.006',
      isActive: true,
      qualityScore: 150,
      totalRewards: '0',
    };
    const errors = validateSchema(station, WEATHER_STATION_SCHEMA);
    expect(errors.some((e) => e.includes('qualityScore'))).toBe(true);
  });

  it('should validate weather data within physical limits', () => {
    const data = {
      stationId: 'WS-NYC-001',
      timestamp: '2025-01-15T10:30:00Z',
      temperature: 2000, // 20.00 C in centidegrees
      humidity: 5000, // 50.00%
      pressure: 101300, // 1013.00 hPa
      windSpeed: 500, // 5.00 m/s
      confidence: 90,
    };
    expect(validateSchema(data, WEATHER_DATA_SCHEMA)).toEqual([]);
  });

  it('should reject weather data outside physical limits', () => {
    const data = {
      stationId: 'WS-NYC-001',
      timestamp: '2025-01-15T10:30:00Z',
      temperature: 99999, // way too hot
      humidity: 5000,
      pressure: 101300,
      windSpeed: 500,
      confidence: 90,
    };
    const errors = validateSchema(data, WEATHER_DATA_SCHEMA);
    expect(errors.some((e) => e.includes('temperature'))).toBe(true);
  });
});

// ---- Carbon Credit API Contracts ----

describe('API Schema Contracts: Carbon Credits', () => {
  const CARBON_CREDIT_SCHEMA: Schema = {
    id: { type: 'string', required: true },
    tokenId: { type: 'string', required: true },
    carbonOffset: { type: 'string', required: true },
    projectType: {
      type: 'string',
      required: true,
      enum: ['weather_monitoring', 'renewable_energy', 'reforestation', 'carbon_capture'],
    },
    verificationStandard: { type: 'string', required: true, enum: ['VCS', 'GS', 'CDM', 'ACR'] },
    issuanceDate: { type: 'string', required: true, pattern: ISO_DATETIME },
    expiryDate: { type: 'string', required: true, pattern: ISO_DATETIME },
    isRetired: { type: 'boolean', required: true },
    owner: { type: 'string', required: true, pattern: ETHEREUM_ADDRESS },
  };

  it('should validate carbon credit schema', () => {
    const credit = {
      id: '00000000-0000-4000-8000-000000000001',
      tokenId: 'CC-1',
      carbonOffset: '10000000000', // 10 tonnes in grams
      projectType: 'weather_monitoring',
      verificationStandard: 'VCS',
      issuanceDate: '2025-01-01T00:00:00Z',
      expiryDate: '2030-01-01T00:00:00Z',
      isRetired: false,
      owner: '0x' + '1'.repeat(40),
    };
    expect(validateSchema(credit, CARBON_CREDIT_SCHEMA)).toEqual([]);
  });

  it('should reject carbon credit with invalid project type', () => {
    const credit = {
      id: 'cc-1',
      tokenId: 'CC-1',
      carbonOffset: '10000000000',
      projectType: 'invalid_type',
      verificationStandard: 'VCS',
      issuanceDate: '2025-01-01T00:00:00Z',
      expiryDate: '2030-01-01T00:00:00Z',
      isRetired: false,
      owner: '0x' + '1'.repeat(40),
    };
    const errors = validateSchema(credit, CARBON_CREDIT_SCHEMA);
    expect(errors.some((e) => e.includes('projectType'))).toBe(true);
  });

  it('should reject carbon credit with invalid verification standard', () => {
    const credit = {
      id: 'cc-1',
      tokenId: 'CC-1',
      carbonOffset: '10000000000',
      projectType: 'weather_monitoring',
      verificationStandard: 'FAKE',
      issuanceDate: '2025-01-01T00:00:00Z',
      expiryDate: '2030-01-01T00:00:00Z',
      isRetired: false,
      owner: '0x' + '1'.repeat(40),
    };
    const errors = validateSchema(credit, CARBON_CREDIT_SCHEMA);
    expect(errors.some((e) => e.includes('verificationStandard'))).toBe(true);
  });
});

// ---- Governance API Contracts ----

describe('API Schema Contracts: Governance', () => {
  const PROPOSAL_SCHEMA: Schema = {
    id: { type: 'string', required: true },
    title: { type: 'string', required: true },
    description: { type: 'string', required: true },
    proposer: { type: 'string', required: true, pattern: ETHEREUM_ADDRESS },
    status: {
      type: 'string',
      required: true,
      enum: ['active', 'passed', 'rejected', 'executed', 'cancelled'],
    },
    votesFor: { type: 'number', required: true, min: 0 },
    votesAgainst: { type: 'number', required: true, min: 0 },
    quorum: { type: 'number', required: true, min: 0 },
  };

  const VOTE_SCHEMA: Schema = {
    proposalId: { type: 'string', required: true },
    voter: { type: 'string', required: true, pattern: ETHEREUM_ADDRESS },
    support: { type: 'boolean', required: true },
    weight: { type: 'number', required: true, min: 0 },
  };

  it('should validate proposal schema', () => {
    const proposal = {
      id: 'prop-1',
      title: 'Increase weather station rewards',
      description: 'Proposal to increase base rewards by 10%',
      proposer: '0x' + '1'.repeat(40),
      status: 'active',
      votesFor: 150,
      votesAgainst: 50,
      quorum: 100,
    };
    expect(validateSchema(proposal, PROPOSAL_SCHEMA)).toEqual([]);
  });

  it('should reject proposal with invalid status', () => {
    const proposal = {
      id: 'prop-1',
      title: 'Test',
      description: 'Test',
      proposer: '0x' + '1'.repeat(40),
      status: 'pending',
      votesFor: 0,
      votesAgainst: 0,
      quorum: 100,
    };
    const errors = validateSchema(proposal, PROPOSAL_SCHEMA);
    expect(errors.some((e) => e.includes('status'))).toBe(true);
  });

  it('should validate vote schema', () => {
    const vote = {
      proposalId: 'prop-1',
      voter: '0x' + '3'.repeat(40),
      support: true,
      weight: 1000,
    };
    expect(validateSchema(vote, VOTE_SCHEMA)).toEqual([]);
  });

  it('should reject vote with negative weight', () => {
    const vote = {
      proposalId: 'prop-1',
      voter: '0x' + '3'.repeat(40),
      support: false,
      weight: -100,
    };
    const errors = validateSchema(vote, VOTE_SCHEMA);
    expect(errors.some((e) => e.includes('weight'))).toBe(true);
  });
});

// ---- Health Check Contract ----

describe('API Schema Contracts: Health Check', () => {
  const HEALTH_SCHEMA: Schema = {
    status: { type: 'string', required: true, enum: ['healthy', 'degraded', 'unhealthy'] },
    service: { type: 'string', required: true },
    uptime: { type: 'number', required: true, min: 0 },
  };

  it('should validate health response schema', () => {
    const health = {
      status: 'healthy',
      service: 'wrfcoin-api',
      uptime: 3600.5,
    };
    expect(validateSchema(health, HEALTH_SCHEMA)).toEqual([]);
  });

  it('should reject health with invalid status', () => {
    const health = {
      status: 'broken',
      service: 'wrfcoin-api',
      uptime: 100,
    };
    const errors = validateSchema(health, HEALTH_SCHEMA);
    expect(errors.some((e) => e.includes('status'))).toBe(true);
  });
});

// ---- Cross-Service Communication Contracts ----

describe('API Schema Contracts: Cross-Service Events', () => {
  const EVENT_SCHEMA: Schema = {
    eventType: {
      type: 'string',
      required: true,
      enum: [
        'weather.data.submitted',
        'weather.station.registered',
        'carbon.credit.minted',
        'carbon.credit.retired',
        'transaction.confirmed',
        'governance.proposal.created',
        'governance.vote.cast',
      ],
    },
    timestamp: { type: 'string', required: true, pattern: ISO_DATETIME },
    source: { type: 'string', required: true },
    payload: { type: 'object', required: true },
  };

  it('should validate event schema for weather data submission', () => {
    const event = {
      eventType: 'weather.data.submitted',
      timestamp: '2025-01-15T10:30:00Z',
      source: 'weather-service',
      payload: { stationId: 'WS-001', temperature: 20.5 },
    };
    expect(validateSchema(event, EVENT_SCHEMA)).toEqual([]);
  });

  it('should validate event schema for carbon credit minting', () => {
    const event = {
      eventType: 'carbon.credit.minted',
      timestamp: '2025-01-15T10:30:00Z',
      source: 'carbon-service',
      payload: { tokenId: 'CC-1', offset: '10.0', owner: '0x123' },
    };
    expect(validateSchema(event, EVENT_SCHEMA)).toEqual([]);
  });

  it('should reject event with unknown event type', () => {
    const event = {
      eventType: 'unknown.event',
      timestamp: '2025-01-15T10:30:00Z',
      source: 'test',
      payload: {},
    };
    const errors = validateSchema(event, EVENT_SCHEMA);
    expect(errors.some((e) => e.includes('eventType'))).toBe(true);
  });

  it('should reject event missing payload', () => {
    const event = {
      eventType: 'transaction.confirmed',
      timestamp: '2025-01-15T10:30:00Z',
      source: 'blockchain-service',
    };
    const errors = validateSchema(event as Record<string, unknown>, EVENT_SCHEMA);
    expect(errors).toContain('Missing required field: payload');
  });
});

// ---- GraphQL Schema Contracts ----

describe('API Schema Contracts: GraphQL Types', () => {
  const GQL_WEATHER_STATION: Schema = {
    id: { type: 'string', required: true },
    address: { type: 'string', required: true, pattern: ETHEREUM_ADDRESS },
    stationId: { type: 'string', required: true },
    location: { type: 'string', required: true },
    isActive: { type: 'boolean', required: true },
    qualityScore: { type: 'number', required: true, min: 0, max: 100 },
  };

  const GQL_CARBON_CREDIT: Schema = {
    id: { type: 'string', required: true },
    carbonOffset: { type: 'string', required: true },
    projectType: { type: 'string', required: true },
    verificationStandard: { type: 'string', required: true },
    isRetired: { type: 'boolean', required: true },
    issuer: { type: 'string', required: true, pattern: ETHEREUM_ADDRESS },
    owner: { type: 'string', required: true, pattern: ETHEREUM_ADDRESS },
    forSale: { type: 'boolean', required: true },
  };

  it('should validate GraphQL WeatherStation type contract', () => {
    const station = {
      id: '1',
      address: '0x' + 'a'.repeat(40),
      stationId: 'WS-001',
      location: '40.7128,-74.006',
      isActive: true,
      qualityScore: 92.5,
    };
    expect(validateSchema(station, GQL_WEATHER_STATION)).toEqual([]);
  });

  it('should validate GraphQL CarbonCredit type contract', () => {
    const credit = {
      id: '1',
      carbonOffset: '10000000000',
      projectType: 'weather_monitoring',
      verificationStandard: 'VCS',
      isRetired: false,
      issuer: '0x' + 'b'.repeat(40),
      owner: '0x' + 'c'.repeat(40),
      forSale: true,
    };
    expect(validateSchema(credit, GQL_CARBON_CREDIT)).toEqual([]);
  });
});
