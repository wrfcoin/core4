/**
 * Type definitions for WRFCoin SDK Extensions
 *
 * Common types and interfaces used across the SDK extensions
 */

// Basic blockchain types
export type Address = string;
export type TransactionHash = string;
export type DateTime = string;
export type BigNumberish = string | number | bigint;

// Pagination and Query types
export interface QueryOptions {
  first?: number;
  skip?: number;
  orderBy?: string;
  orderDirection?: 'asc' | 'desc';
}

export interface QueryResponse<T> {
  data: T[];
  totalCount: number;
}

// Weather Station types
export interface WeatherStation {
  id: string;
  address: Address;
  stationId: string;
  location: string;
  isActive: boolean;
  lastRewardTime?: DateTime;
  totalRewards: BigNumberish;
  qualityScore: number;
  latestWeatherData?: WeatherData;
}

export interface WeatherStationFilter {
  isActive?: boolean;
  qualityScore_gte?: number;
  location_contains?: string;
}

// Weather Data types
export interface WeatherData {
  id: string;
  stationId: string;
  timestamp: DateTime;
  temperature: number; // in centidegrees Celsius
  humidity: number; // percentage (0-100)
  pressure: number; // in centihPa
  windSpeed: number; // in cm/s
  rainfall: number; // in centimm
  confidence: number; // percentage (0-100)
  station?: WeatherStation;
}

export interface WeatherDataInput {
  stationId: string;
  signature: string;
  temperature: number; // in degrees Celsius
  humidity: number; // percentage (0-100)
  pressure: number; // in hPa
  windSpeed: number; // in m/s
  rainfall: number; // in mm
  confidence: number; // percentage (0-100)
}

export interface WeatherDataSubmissionResult {
  success: boolean;
  weatherData?: WeatherData;
  rewards?: BigNumberish;
  error?: string;
}

export interface WeatherMeasurement {
  timestamp: DateTime;
  temperature: number;
  humidity: number;
  pressure: number;
  windSpeed: number;
  rainfall: number;
  confidence: number;
}

// IoT Device types
export interface IoTDevice {
  id: string;
  deviceId: string;
  deviceType: 'weather_station' | 'air_quality_monitor' | 'soil_sensor';
  owner: Address;
  location: {
    latitude: number;
    longitude: number;
    address?: string;
  };
  isActive: boolean;
  lastDataSubmission?: DateTime;
  totalRewards: BigNumberish;
  qualityScore: number;
}

export interface DeviceConfiguration {
  deviceId: string;
  calibrationData?: Record<string, any>;
  sampleRate?: number; // seconds between readings
  uploadInterval?: number; // seconds between uploads
  qualityThreshold?: number; // minimum confidence score
}

// Governance types
export interface Proposal {
  id: string;
  title: string;
  description: string;
  proposer: Address;
  status: 'pending' | 'active' | 'succeeded' | 'defeated' | 'executed';
  votingStartTime?: DateTime;
  votingEndTime?: DateTime;
  forVotes: BigNumberish;
  againstVotes: BigNumberish;
  abstainVotes: BigNumberish;
}

export interface Vote {
  id: string;
  proposalId: string;
  voter: Address;
  support: 'for' | 'against' | 'abstain';
  weight: BigNumberish;
  reason?: string;
  timestamp: DateTime;
}

// NFT types
export interface ComputationalNFT {
  id: string;
  tokenId: string;
  owner: Address;
  metadata: {
    name: string;
    description: string;
    image?: string;
    computationType: string;
    difficulty: number;
    requiredResources: {
      cpu: number;
      memory: number;
      storage: number;
    };
  };
  status: 'pending' | 'computing' | 'completed' | 'failed';
  createdAt: DateTime;
  completedAt?: DateTime;
}

// Mining and Rewards types
export interface MiningReward {
  id: string;
  recipient: Address;
  amount: BigNumberish;
  reason: 'weather_data' | 'computation' | 'validation' | 'governance';
  blockNumber: number;
  transactionHash: TransactionHash;
  timestamp: DateTime;
}

export interface StakingPosition {
  id: string;
  staker: Address;
  amount: BigNumberish;
  startTime: DateTime;
  endTime?: DateTime;
  rewards: BigNumberish;
  isActive: boolean;
}

// Configuration types
export interface SDKConfig {
  apiEndpoint: string;
  wsEndpoint?: string;
  networkId: number;
  contractAddresses: {
    token: Address;
    governance: Address;
    staking: Address;
    weatherNFT: Address;
    computationalNFT: Address;
  };
  defaultGasPrice?: BigNumberish;
  timeout?: number;
  retries?: number;
  debug?: boolean;
}

// Event types
export interface BlockchainEvent {
  id: string;
  transactionHash: TransactionHash;
  blockNumber: number;
  eventName: string;
  contractAddress: Address;
  args: Record<string, any>;
  timestamp: DateTime;
}

// Utility types
export type ChainId = number;
export type TokenSymbol = string;
export type NetworkName = string;

export interface TokenInfo {
  symbol: TokenSymbol;
  name: string;
  decimals: number;
  address: Address;
  chainId: ChainId;
}

export interface NetworkInfo {
  name: NetworkName;
  chainId: ChainId;
  rpcUrl: string;
  blockExplorerUrl?: string;
  nativeCurrency: TokenInfo;
}

// Error types
export interface ErrorDetails {
  code?: string;
  field?: string;
  message: string;
  details?: any;
}

// WebSocket subscription types
export interface SubscriptionOptions {
  onData?: (data: any) => void;
  onError?: (error: Error) => void;
  onComplete?: () => void;
}

// API Response types
export interface ApiResponse<T = any> {
  success: boolean;
  data?: T;
  error?: ErrorDetails;
  timestamp: DateTime;
}

export interface PaginatedResponse<T = any> extends ApiResponse<T[]> {
  pagination?: {
    page: number;
    limit: number;
    total: number;
    hasNext: boolean;
    hasPrev: boolean;
  };
}
