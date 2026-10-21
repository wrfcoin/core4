/**
 * Test Data Factories
 *
 * Provides reusable factory functions for creating test data objects
 * that match the application's Drizzle schema. Each factory produces
 * valid default data that can be overridden with partial inputs.
 *
 * Usage:
 *   import { createUser, createTransaction, createWeatherStation } from '../factories';
 *   const user = createUser({ username: 'custom-name' });
 *   const tx = createTransaction({ amount: 500 });
 */

let counter = 0;
function nextId(): number {
  return ++counter;
}

function uuid(): string {
  // Simple deterministic UUID-like string for testing
  const id = nextId();
  return `00000000-0000-4000-8000-${id.toString().padStart(12, '0')}`;
}

function walletAddress(): string {
  const id = nextId();
  return `0x${id.toString(16).padStart(40, '0')}`;
}

function txHash(): string {
  const id = nextId();
  return `0x${id.toString(16).padStart(64, 'a')}`;
}

// ---- Users ----

export interface TestUser {
  id: number;
  username: string;
  email: string | null;
  walletAddress: string;
  createdAt: Date;
  updatedAt: Date;
  lastLogin: Date | null;
  status: string;
  role: string;
  preferences: Record<string, unknown> | null;
  reputation: number;
  isVerified: boolean;
}

export function createUser(overrides: Partial<TestUser> = {}): TestUser {
  const id = overrides.id ?? nextId();
  return {
    id,
    username: `test-user-${id}`,
    email: `user${id}@test.wrfcoin.local`,
    walletAddress: walletAddress(),
    createdAt: new Date('2025-01-01T00:00:00Z'),
    updatedAt: new Date('2025-01-01T00:00:00Z'),
    lastLogin: null,
    status: 'active',
    role: 'user',
    preferences: null,
    reputation: 0,
    isVerified: false,
    ...overrides,
  };
}

export function createAdminUser(overrides: Partial<TestUser> = {}): TestUser {
  return createUser({
    role: 'admin',
    isVerified: true,
    reputation: 100,
    ...overrides,
  });
}

// ---- Transactions ----

export interface TestTransaction {
  id: number;
  txId: string;
  fromAddress: string;
  toAddress: string;
  amount: number;
  fee: number;
  timestamp: Date;
  status: string;
  type: string;
  blockId: number | null;
  data: Record<string, unknown> | null;
}

export function createTransaction(overrides: Partial<TestTransaction> = {}): TestTransaction {
  const id = overrides.id ?? nextId();
  return {
    id,
    txId: txHash(),
    fromAddress: walletAddress(),
    toAddress: walletAddress(),
    amount: 100,
    fee: 1,
    timestamp: new Date('2025-01-01T00:00:00Z'),
    status: 'confirmed',
    type: 'transfer',
    blockId: null,
    data: null,
    ...overrides,
  };
}

// ---- Weather Stations ----

export interface TestWeatherStation {
  id: string;
  name: string;
  location: {
    lat: number;
    lng: number;
    altitude?: number;
    country?: string;
  };
  owner: string;
  status: string;
  platform: string;
  qualityScore: number;
  createdAt: Date;
  lastDataAt: Date | null;
  metadata: Record<string, unknown> | null;
}

export function createWeatherStation(
  overrides: Partial<TestWeatherStation> = {}
): TestWeatherStation {
  const id = overrides.id ?? `station-${nextId()}`;
  return {
    id,
    name: `Test Station ${id}`,
    location: {
      lat: 40.7128 + Math.random() * 0.01,
      lng: -74.006 + Math.random() * 0.01,
      altitude: 10,
      country: 'US',
    },
    owner: walletAddress(),
    status: 'active',
    platform: 'acurite',
    qualityScore: 85,
    createdAt: new Date('2025-01-01T00:00:00Z'),
    lastDataAt: null,
    metadata: null,
    ...overrides,
  };
}

// ---- Weather Data ----

export interface TestWeatherData {
  stationId: string;
  timestamp: Date;
  temperature: number;
  humidity: number;
  pressure: number;
  windSpeed: number;
  windDirection: number;
  precipitation: number;
  qualityScore: number;
}

export function createWeatherData(overrides: Partial<TestWeatherData> = {}): TestWeatherData {
  return {
    stationId: `station-${nextId()}`,
    timestamp: new Date(),
    temperature: 20 + Math.random() * 10, // 20-30 C
    humidity: 40 + Math.random() * 30, // 40-70%
    pressure: 1000 + Math.random() * 20, // 1000-1020 hPa
    windSpeed: Math.random() * 15, // 0-15 m/s
    windDirection: Math.random() * 360,
    precipitation: 0,
    qualityScore: 80 + Math.random() * 15,
    ...overrides,
  };
}

// ---- Carbon Credits ----

export interface TestCarbonCredit {
  id: string;
  tokenId: string;
  owner: string;
  carbonOffset: string;
  projectType: string;
  verificationStandard: string;
  issuanceDate: Date;
  expiryDate: Date | null;
  location: { country: string; region: string } | null;
  isRetired: boolean;
}

export function createCarbonCredit(overrides: Partial<TestCarbonCredit> = {}): TestCarbonCredit {
  const id = overrides.id ?? uuid();
  return {
    id,
    tokenId: `CC-${nextId()}`,
    owner: walletAddress(),
    carbonOffset: '10.00000000',
    projectType: 'weather_monitoring',
    verificationStandard: 'VCS',
    issuanceDate: new Date('2025-01-01T00:00:00Z'),
    expiryDate: new Date('2030-01-01T00:00:00Z'),
    location: { country: 'US', region: 'Northeast' },
    isRetired: false,
    ...overrides,
  };
}

// ---- Blocks ----

export interface TestBlock {
  id: number;
  blockId: string;
  height: number;
  timestamp: Date;
  hash: string;
  prevHash: string;
  transactionCount: number;
  difficulty: number;
  size: number;
  validator: string | null;
  merkleRoot: string;
}

export function createBlock(overrides: Partial<TestBlock> = {}): TestBlock {
  const id = overrides.id ?? nextId();
  return {
    id,
    blockId: `block-${id}`,
    height: id,
    timestamp: new Date('2025-01-01T00:00:00Z'),
    hash: txHash(),
    prevHash: txHash(),
    transactionCount: 0,
    difficulty: 1,
    size: 256,
    validator: null,
    merkleRoot: txHash(),
    ...overrides,
  };
}

// ---- Governance Proposals ----

export interface TestProposal {
  id: string;
  title: string;
  description: string;
  proposer: string;
  status: string;
  votingStart: Date;
  votingEnd: Date;
  votesFor: number;
  votesAgainst: number;
  quorum: number;
}

export function createProposal(overrides: Partial<TestProposal> = {}): TestProposal {
  const id = overrides.id ?? `prop-${nextId()}`;
  return {
    id,
    title: `Test Proposal ${id}`,
    description: 'A test governance proposal',
    proposer: walletAddress(),
    status: 'active',
    votingStart: new Date('2025-01-01T00:00:00Z'),
    votingEnd: new Date('2025-01-08T00:00:00Z'),
    votesFor: 0,
    votesAgainst: 0,
    quorum: 100,
    ...overrides,
  };
}

// ---- Batch Helpers ----

export function createUsers(count: number, overrides: Partial<TestUser> = {}): TestUser[] {
  return Array.from({ length: count }, () => createUser(overrides));
}

export function createTransactions(
  count: number,
  overrides: Partial<TestTransaction> = {}
): TestTransaction[] {
  return Array.from({ length: count }, () => createTransaction(overrides));
}

export function createWeatherStations(
  count: number,
  overrides: Partial<TestWeatherStation> = {}
): TestWeatherStation[] {
  return Array.from({ length: count }, () => createWeatherStation(overrides));
}

// ---- Reset ----

/** Reset the internal counter (call in beforeEach for deterministic tests) */
export function resetFactories(): void {
  counter = 0;
}
