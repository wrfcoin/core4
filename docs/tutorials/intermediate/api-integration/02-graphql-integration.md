# GraphQL API Integration Guide

Complete guide to integrating with WRFCoin's GraphQL API for efficient data querying and real-time
subscriptions.

## What You'll Learn

- GraphQL fundamentals and WRFCoin schema
- Advanced query patterns and optimization
- Real-time subscriptions for live data
- Error handling and performance optimization
- Client-side caching strategies
- Best practices for production applications

## Prerequisites

- Completed [REST API Integration](./01-rest-api-integration.md)
- Basic understanding of GraphQL concepts
- Experience with JavaScript/TypeScript
- Familiarity with modern frontend frameworks

---

## 1. GraphQL Client Setup

### Apollo Client Configuration

```javascript
// config/apollo-client.js
import { ApolloClient, InMemoryCache, createHttpLink, from, split } from '@apollo/client';
import { setContext } from '@apollo/client/link/context';
import { onError } from '@apollo/client/link/error';
import { RetryLink } from '@apollo/client/link/retry';
import { WebSocketLink } from '@apollo/client/link/ws';
import { getMainDefinition } from '@apollo/client/utilities';
import crypto from 'crypto';

class WRFCoinGraphQLClient {
  constructor(config = {}) {
    this.config = {
      httpUri: config.httpUri || 'https://graphql.wrfcoin.network/graphql',
      wsUri: config.wsUri || 'wss://graphql.wrfcoin.network/graphql',
      apiKey: config.apiKey || process.env.WRFCOIN_API_KEY,
      apiSecret: config.apiSecret || process.env.WRFCOIN_API_SECRET,
      network: config.network || 'mainnet',
      ...config,
    };

    this.client = this._createApolloClient();
  }

  _createApolloClient() {
    // HTTP Link for queries and mutations
    const httpLink = createHttpLink({
      uri: this.config.httpUri,
      headers: {
        'X-Network': this.config.network,
      },
    });

    // WebSocket Link for subscriptions
    const wsLink = new WebSocketLink({
      uri: this.config.wsUri,
      options: {
        reconnect: true,
        connectionParams: () => ({
          authToken: this._generateAuthToken(),
          network: this.config.network,
        }),
        connectionCallback: error => {
          if (error) {
            console.error('❌ WebSocket connection error:', error);
          } else {
            console.log('✅ WebSocket connected for GraphQL subscriptions');
          }
        },
      },
    });

    // Authentication Link
    const authLink = setContext((_, { headers }) => {
      const authToken = this._generateAuthToken();

      return {
        headers: {
          ...headers,
          authorization: authToken ? `Bearer ${authToken}` : '',
          'x-api-key': this.config.apiKey,
          'x-timestamp': Date.now().toString(),
          'x-signature': this._generateSignature(),
        },
      };
    });

    // Error Link
    const errorLink = onError(({ graphQLErrors, networkError, operation, forward }) => {
      if (graphQLErrors) {
        graphQLErrors.forEach(({ message, locations, path, extensions }) => {
          console.error(
            `❌ GraphQL error: Message: ${message}, Location: ${locations}, Path: ${path}`
          );

          // Handle specific error types
          if (extensions?.code === 'UNAUTHENTICATED') {
            console.error('🔐 Authentication error - check API credentials');
          } else if (extensions?.code === 'RATE_LIMITED') {
            console.warn('⚠️  Rate limit exceeded - retrying...');
          }
        });
      }

      if (networkError) {
        console.error(`❌ Network error: ${networkError}`);

        // Handle network errors
        if (networkError.statusCode === 401) {
          console.error('🔐 Authentication failed');
        } else if (networkError.statusCode === 429) {
          console.warn('⚠️  Rate limited');
        }
      }
    });

    // Retry Link
    const retryLink = new RetryLink({
      delay: {
        initial: 1000,
        max: 5000,
        jitter: true,
      },
      attempts: {
        max: 3,
        retryIf: (error, _operation) => {
          return !!error && error.statusCode !== 401; // Don't retry auth errors
        },
      },
    });

    // Combine HTTP and WebSocket links
    const splitLink = split(
      ({ query }) => {
        const definition = getMainDefinition(query);
        return definition.kind === 'OperationDefinition' && definition.operation === 'subscription';
      },
      wsLink,
      httpLink
    );

    // Create cache with type policies
    const cache = new InMemoryCache({
      typePolicies: {
        Query: {
          fields: {
            climateData: {
              keyArgs: ['filter', 'sort'],
              merge(existing = [], incoming, { args }) {
                if (args?.pagination?.offset === 0) {
                  return incoming;
                }
                return [...existing, ...incoming];
              },
            },
            transactions: {
              keyArgs: ['filter'],
              merge(existing = [], incoming, { args }) {
                if (args?.pagination?.offset === 0) {
                  return incoming;
                }
                return [...existing, ...incoming];
              },
            },
          },
        },
        ClimateData: {
          keyFields: ['id'],
        },
        Transaction: {
          keyFields: ['hash'],
        },
        WeatherStation: {
          keyFields: ['id'],
        },
        Block: {
          keyFields: ['number'],
        },
      },
    });

    // Combine all links
    const link = from([errorLink, retryLink, authLink, splitLink]);

    return new ApolloClient({
      link,
      cache,
      defaultOptions: {
        watchQuery: {
          errorPolicy: 'all',
          fetchPolicy: 'cache-and-network',
        },
        query: {
          errorPolicy: 'all',
          fetchPolicy: 'cache-first',
        },
        mutate: {
          errorPolicy: 'all',
        },
      },
    });
  }

  _generateAuthToken() {
    // Generate JWT-like token with API key
    const payload = {
      apiKey: this.config.apiKey,
      timestamp: Date.now(),
      network: this.config.network,
    };

    return Buffer.from(JSON.stringify(payload)).toString('base64');
  }

  _generateSignature() {
    const timestamp = Date.now().toString();
    const message = `${this.config.apiKey}${timestamp}`;

    return crypto.createHmac('sha256', this.config.apiSecret).update(message).digest('hex');
  }

  // Public methods
  getClient() {
    return this.client;
  }

  async query(query, variables = {}, options = {}) {
    try {
      const result = await this.client.query({
        query,
        variables,
        ...options,
      });
      return result;
    } catch (error) {
      console.error('❌ GraphQL query failed:', error);
      throw error;
    }
  }

  async mutate(mutation, variables = {}, options = {}) {
    try {
      const result = await this.client.mutate({
        mutation,
        variables,
        ...options,
      });
      return result;
    } catch (error) {
      console.error('❌ GraphQL mutation failed:', error);
      throw error;
    }
  }

  subscribe(subscription, variables = {}, options = {}) {
    return this.client.subscribe({
      query: subscription,
      variables,
      ...options,
    });
  }
}

export default WRFCoinGraphQLClient;
```

---

## 2. Schema and Query Patterns

### Core Schema Types

```graphql
# schema/types.graphql

type Query {
  # Network queries
  networkInfo: NetworkInfo!
  blocks(filter: BlockFilter, pagination: Pagination, sort: Sort): [Block!]!
  block(number: Int, hash: String): Block

  # Transaction queries
  transactions(filter: TransactionFilter, pagination: Pagination, sort: Sort): [Transaction!]!
  transaction(hash: String!): Transaction

  # Address queries
  address(address: String!): Address
  addressTransactions(
    address: String!
    filter: TransactionFilter
    pagination: Pagination
  ): [Transaction!]!

  # Climate data queries
  climateData(filter: ClimateDataFilter, pagination: Pagination, sort: Sort): [ClimateData!]!
  weatherStations(filter: WeatherStationFilter, pagination: Pagination): [WeatherStation!]!
  weatherStation(id: ID!): WeatherStation

  # Token and DeFi queries
  tokens(filter: TokenFilter, pagination: Pagination): [Token!]!
  token(address: String!): Token
  stakingInfo(address: String): StakingInfo

  # Governance queries
  governanceProposals(filter: ProposalFilter, pagination: Pagination): [GovernanceProposal!]!
  governanceProposal(id: ID!): GovernanceProposal

  # Analytics queries
  networkStats(timeframe: Timeframe!): NetworkStats!
  climateStats(region: String, timeframe: Timeframe!): ClimateStats!
  tokenStats(tokenAddress: String!, timeframe: Timeframe!): TokenStats!
}

type Mutation {
  # Climate data mutations
  submitClimateData(input: ClimateDataInput!): ClimateDataSubmissionResult!
  registerWeatherStation(input: WeatherStationInput!): WeatherStationRegistrationResult!

  # Governance mutations
  createProposal(input: ProposalInput!): ProposalCreationResult!
  vote(proposalId: ID!, vote: VoteChoice!, power: String): VoteResult!

  # Staking mutations
  stake(amount: String!): StakingResult!
  unstake(amount: String!): UnstakingResult!

  # Oracle mutations
  submitValidation(dataId: ID!, approved: Boolean!, reason: String): ValidationResult!
}

type Subscription {
  # Real-time data subscriptions
  newBlocks: Block!
  newTransactions(filter: TransactionFilter): Transaction!
  newClimateData(filter: ClimateDataFilter): ClimateData!

  # Address-specific subscriptions
  addressActivity(address: String!): AddressActivity!

  # Governance subscriptions
  proposalUpdates(proposalId: ID): GovernanceProposalUpdate!
  votingActivity: VotingActivity!

  # Network subscriptions
  networkEvents: NetworkEvent!
  validationEvents: ValidationEvent!
}

# Core types
type NetworkInfo {
  name: String!
  chainId: Int!
  blockHeight: Int!
  blockTime: Float!
  totalTransactions: String!
  activeValidators: Int!
  stakingRatio: Float!
  totalSupply: String!
  circulatingSupply: String!
}

type Block {
  number: Int!
  hash: String!
  parentHash: String!
  timestamp: String!
  miner: String!
  gasUsed: String!
  gasLimit: String!
  transactions: [Transaction!]!
  transactionCount: Int!
  size: String!
  difficulty: String!
}

type Transaction {
  hash: String!
  blockNumber: Int!
  blockHash: String!
  index: Int!
  from: String!
  to: String
  value: String!
  gas: String!
  gasPrice: String!
  gasUsed: String
  status: TransactionStatus!
  timestamp: String!
  input: String
  logs: [Log!]!
  climate: ClimateTransaction
}

type ClimateData {
  id: ID!
  submitter: String!
  stationId: String
  timestamp: String!
  location: GeoLocation!
  measurements: ClimateParameters!
  validation: ValidationInfo
  rewards: RewardInfo
  ipfsHash: String
  onChainTxHash: String
}

type WeatherStation {
  id: ID!
  name: String!
  description: String
  owner: String!
  location: GeoLocation!
  deviceInfo: DeviceInfo!
  capabilities: [String!]!
  status: StationStatus!
  registrationDate: String!
  lastDataSubmission: String
  totalSubmissions: Int!
  validationAccuracy: Float
}

type GovernanceProposal {
  id: ID!
  title: String!
  description: String!
  proposer: String!
  status: ProposalStatus!
  type: ProposalType!
  votingStartTime: String!
  votingEndTime: String!
  executionTime: String
  forVotes: String!
  againstVotes: String!
  abstainVotes: String!
  totalVotingPower: String!
  quorumReached: Boolean!
  executed: Boolean!
  votes: [Vote!]!
}

# Input types
input ClimateDataInput {
  stationId: String
  timestamp: String
  location: GeoLocationInput!
  measurements: ClimateParametersInput!
  deviceInfo: DeviceInfoInput
  signature: String!
}

input ClimateDataFilter {
  startDate: String
  endDate: String
  location: GeoLocationFilter
  dataTypes: [ClimateDataType!]
  stationIds: [String!]
  validated: Boolean
  submitter: String
}

input WeatherStationFilter {
  location: GeoLocationFilter
  status: StationStatus
  capabilities: [String!]
  owner: String
}

input TransactionFilter {
  from: String
  to: String
  minValue: String
  maxValue: String
  startDate: String
  endDate: String
  status: TransactionStatus
  type: TransactionType
}

# Enums
enum TransactionStatus {
  PENDING
  CONFIRMED
  FAILED
}

enum StationStatus {
  ACTIVE
  INACTIVE
  MAINTENANCE
  DECOMMISSIONED
}

enum ProposalStatus {
  PENDING
  ACTIVE
  SUCCEEDED
  DEFEATED
  EXECUTED
  CANCELLED
}

enum ClimateDataType {
  TEMPERATURE
  HUMIDITY
  PRESSURE
  WIND_SPEED
  WIND_DIRECTION
  PRECIPITATION
  UV_INDEX
  AIR_QUALITY
}
```

### Advanced Query Examples

```javascript
// queries/advanced-queries.js
import { gql } from '@apollo/client';

// Complex climate data query with filtering and aggregation
export const GET_CLIMATE_DATA_ANALYTICS = gql`
  query GetClimateDataAnalytics(
    $filter: ClimateDataFilter!,
    $timeframe: Timeframe!,
    $aggregation: AggregationType!
  ) {
    climateData(filter: $filter) {
      id
      timestamp
      location {
        latitude
        longitude
        elevation
      }
      measurements {
        temperature
        humidity
        pressure
        windSpeed
        windDirection
        precipitation
      }
      validation {
        status
        accuracy
        validatedAt
        validators {
          address
          stake
        }
      }
      submitter
      stationId
    }
    
    climateStats(
      region: $filter.location.region,
      timeframe: $timeframe
    ) {
      averageTemperature
      temperatureRange {
        min
        max
      }
      totalPrecipitation
      dataPointCount
      stationCount
      aggregationType: $aggregation
      trends {
        temperature {
          direction
          changeRate
          confidence
        }
        precipitation {
          direction
          changeRate
          confidence
        }
      }
    }
  }
`;

// Real-time dashboard query
export const GET_DASHBOARD_DATA = gql`
  query GetDashboardData {
    networkInfo {
      name
      chainId
      blockHeight
      blockTime
      totalTransactions
      activeValidators
      stakingRatio
    }

    recentBlocks: blocks(
      pagination: { limit: 5, offset: 0 }
      sort: { field: "number", direction: DESC }
    ) {
      number
      hash
      timestamp
      transactionCount
      gasUsed
      miner
    }

    recentTransactions: transactions(
      filter: { minValue: "1000" }
      pagination: { limit: 10, offset: 0 }
      sort: { field: "timestamp", direction: DESC }
    ) {
      hash
      from
      to
      value
      timestamp
      status
      climate {
        dataType
        stationId
      }
    }

    recentClimateData: climateData(
      pagination: { limit: 10, offset: 0 }
      sort: { field: "timestamp", direction: DESC }
    ) {
      id
      timestamp
      stationId
      location {
        latitude
        longitude
      }
      measurements {
        temperature
        humidity
        pressure
      }
      validation {
        status
        accuracy
      }
    }

    activeProposals: governanceProposals(
      filter: { status: ACTIVE }
      pagination: { limit: 5, offset: 0 }
    ) {
      id
      title
      status
      type
      votingEndTime
      forVotes
      againstVotes
      quorumReached
    }
  }
`;

// Weather station performance query
export const GET_STATION_PERFORMANCE = gql`
  query GetStationPerformance($stationId: ID!, $timeframe: Timeframe!) {
    weatherStation(id: $stationId) {
      id
      name
      location {
        latitude
        longitude
        elevation
        address
      }
      deviceInfo {
        model
        manufacturer
        firmwareVersion
        lastCalibration
      }
      status
      totalSubmissions
      validationAccuracy

      recentData: climateData(
        filter: { stationIds: [$stationId] }
        pagination: { limit: 100, offset: 0 }
        sort: { field: "timestamp", direction: DESC }
      ) {
        id
        timestamp
        measurements {
          temperature
          humidity
          pressure
          windSpeed
          windDirection
          precipitation
        }
        validation {
          status
          accuracy
          validatedAt
        }
      }
    }

    stationStats: climateStats(region: $stationId, timeframe: $timeframe) {
      dataPointCount
      validationRate
      averageAccuracy
      uptimePercentage
      rewards {
        total
        perValidation
        perSubmission
      }
    }
  }
`;

// Token holder analysis query
export const GET_TOKEN_HOLDERS_ANALYSIS = gql`
  query GetTokenHoldersAnalysis($tokenAddress: String!, $timeframe: Timeframe!) {
    token(address: $tokenAddress) {
      name
      symbol
      decimals
      totalSupply
      holders {
        count
        distribution {
          range
          count
          percentage
        }
      }

      topHolders: holders(
        pagination: { limit: 20, offset: 0 }
        sort: { field: "balance", direction: DESC }
      ) {
        address
        balance
        percentage
        firstActivity
        lastActivity
        transactionCount
      }
    }

    tokenStats(tokenAddress: $tokenAddress, timeframe: $timeframe) {
      priceHistory {
        timestamp
        price
        volume
        marketCap
      }
      transferVolume
      uniqueTransfers
      burnEvents {
        timestamp
        amount
        reason
      }
      mintEvents {
        timestamp
        amount
        recipient
        reason
      }
    }
  }
`;

// Governance participation analysis
export const GET_GOVERNANCE_PARTICIPATION = gql`
  query GetGovernanceParticipation($timeframe: Timeframe!) {
    governanceProposals(filter: { timeframe: $timeframe }, pagination: { limit: 100, offset: 0 }) {
      id
      title
      type
      status
      proposer
      votingStartTime
      votingEndTime
      forVotes
      againstVotes
      abstainVotes
      totalVotingPower
      quorumReached
      executed

      votes {
        voter
        choice
        power
        timestamp
        reason
      }

      participation {
        voterCount
        participationRate
        averageVotingPower
        topVoters {
          address
          totalPower
          voteCount
        }
      }
    }

    governanceStats: networkStats(timeframe: $timeframe) {
      governance {
        totalProposals
        executedProposals
        averageParticipation
        totalVotingPower
        uniqueVoters
        proposalSuccessRate
      }
    }
  }
`;
```

---

## 3. Real-Time Subscriptions

### Live Data Subscriptions

```javascript
// subscriptions/live-subscriptions.js
import { gql } from '@apollo/client';

// Real-time block subscription
export const SUBSCRIBE_TO_NEW_BLOCKS = gql`
  subscription SubscribeToNewBlocks {
    newBlocks {
      number
      hash
      timestamp
      transactionCount
      gasUsed
      gasLimit
      miner
      transactions {
        hash
        from
        to
        value
        climate {
          dataType
          stationId
        }
      }
    }
  }
`;

// Climate data subscription with filtering
export const SUBSCRIBE_TO_CLIMATE_DATA = gql`
  subscription SubscribeToClimateData($filter: ClimateDataFilter) {
    newClimateData(filter: $filter) {
      id
      timestamp
      stationId
      submitter
      location {
        latitude
        longitude
        elevation
      }
      measurements {
        temperature
        humidity
        pressure
        windSpeed
        windDirection
        precipitation
        uvIndex
        airQuality {
          aqi
          pm25
          pm10
          o3
          no2
          so2
          co
        }
      }
      validation {
        status
        requestedAt
        validators {
          address
          stake
        }
      }
      onChainTxHash
    }
  }
`;

// Address activity subscription
export const SUBSCRIBE_TO_ADDRESS_ACTIVITY = gql`
  subscription SubscribeToAddressActivity($address: String!) {
    addressActivity(address: $address) {
      type
      timestamp
      transaction {
        hash
        from
        to
        value
        status
        climate {
          dataType
          stationId
          dataId
        }
      }
      balance {
        current
        previous
        change
      }
      tokens {
        address
        symbol
        balance
        change
      }
      staking {
        staked
        rewards
        delegations {
          validator
          amount
        }
      }
    }
  }
`;

// Governance subscription
export const SUBSCRIBE_TO_GOVERNANCE_UPDATES = gql`
  subscription SubscribeToGovernanceUpdates($proposalId: ID) {
    proposalUpdates(proposalId: $proposalId) {
      proposal {
        id
        title
        status
        forVotes
        againstVotes
        abstainVotes
        quorumReached
      }
      update {
        type
        timestamp
        details
        vote {
          voter
          choice
          power
          reason
        }
      }
    }
  }
`;

// Network events subscription
export const SUBSCRIBE_TO_NETWORK_EVENTS = gql`
  subscription SubscribeToNetworkEvents {
    networkEvents {
      type
      timestamp
      severity
      message
      details

      ... on ValidatorEvent {
        validator
        eventType
        stake
        performance
      }

      ... on NetworkUpgradeEvent {
        version
        blockNumber
        features
      }

      ... on EmergencyEvent {
        alertLevel
        action
        affectedServices
      }
    }
  }
`;
```

### Subscription Hook Implementation

```javascript
// hooks/useWRFCoinSubscriptions.js
import { useSubscription, useQuery } from '@apollo/client';
import { useState, useEffect, useCallback } from 'react';

// Custom hook for managing multiple subscriptions
export function useWRFCoinSubscriptions(config = {}) {
  const [subscriptions, setSubscriptions] = useState(new Map());
  const [connectionStatus, setConnectionStatus] = useState('disconnected');
  const [errors, setErrors] = useState([]);

  // Subscribe to new blocks
  const { data: newBlockData, error: blockError } = useSubscription(SUBSCRIBE_TO_NEW_BLOCKS, {
    onSubscriptionData: ({ subscriptionData }) => {
      if (subscriptionData.data?.newBlocks) {
        config.onNewBlock?.(subscriptionData.data.newBlocks);
      }
    },
    onError: error => {
      console.error('Block subscription error:', error);
      setErrors(prev => [...prev, { type: 'block', error }]);
    },
  });

  // Subscribe to climate data
  const { data: climateData, error: climateError } = useSubscription(SUBSCRIBE_TO_CLIMATE_DATA, {
    variables: { filter: config.climateFilter },
    skip: !config.climateFilter,
    onSubscriptionData: ({ subscriptionData }) => {
      if (subscriptionData.data?.newClimateData) {
        config.onNewClimateData?.(subscriptionData.data.newClimateData);
      }
    },
    onError: error => {
      console.error('Climate data subscription error:', error);
      setErrors(prev => [...prev, { type: 'climate', error }]);
    },
  });

  // Subscribe to address activity
  const { data: addressData, error: addressError } = useSubscription(
    SUBSCRIBE_TO_ADDRESS_ACTIVITY,
    {
      variables: { address: config.watchAddress },
      skip: !config.watchAddress,
      onSubscriptionData: ({ subscriptionData }) => {
        if (subscriptionData.data?.addressActivity) {
          config.onAddressActivity?.(subscriptionData.data.addressActivity);
        }
      },
      onError: error => {
        console.error('Address subscription error:', error);
        setErrors(prev => [...prev, { type: 'address', error }]);
      },
    }
  );

  // Subscribe to governance updates
  const { data: governanceData, error: governanceError } = useSubscription(
    SUBSCRIBE_TO_GOVERNANCE_UPDATES,
    {
      variables: { proposalId: config.proposalId },
      skip: !config.enableGovernance,
      onSubscriptionData: ({ subscriptionData }) => {
        if (subscriptionData.data?.proposalUpdates) {
          config.onGovernanceUpdate?.(subscriptionData.data.proposalUpdates);
        }
      },
      onError: error => {
        console.error('Governance subscription error:', error);
        setErrors(prev => [...prev, { type: 'governance', error }]);
      },
    }
  );

  // Monitor connection status
  useEffect(() => {
    const hasErrors = errors.length > 0;
    const hasData = newBlockData || climateData || addressData || governanceData;

    if (hasErrors) {
      setConnectionStatus('error');
    } else if (hasData) {
      setConnectionStatus('connected');
    } else {
      setConnectionStatus('connecting');
    }
  }, [newBlockData, climateData, addressData, governanceData, errors]);

  // Clear errors
  const clearErrors = useCallback(() => {
    setErrors([]);
  }, []);

  // Get subscription statistics
  const getStats = useCallback(() => {
    return {
      connectionStatus,
      activeSubscriptions: subscriptions.size,
      errors: errors.length,
      lastUpdate: Math.max(
        newBlockData?.timestamp || 0,
        climateData?.timestamp || 0,
        addressData?.timestamp || 0,
        governanceData?.timestamp || 0
      ),
    };
  }, [
    connectionStatus,
    subscriptions.size,
    errors.length,
    newBlockData,
    climateData,
    addressData,
    governanceData,
  ]);

  return {
    connectionStatus,
    errors,
    clearErrors,
    getStats,

    // Subscription data
    newBlock: newBlockData?.newBlocks,
    newClimateData: climateData?.newClimateData,
    addressActivity: addressData?.addressActivity,
    governanceUpdate: governanceData?.proposalUpdates,
  };
}

// Hook for specific climate monitoring
export function useClimateMonitoring(stationIds = [], region = null) {
  const [climateUpdates, setClimateUpdates] = useState([]);
  const [alerts, setAlerts] = useState([]);

  const filter = {
    stationIds: stationIds.length > 0 ? stationIds : undefined,
    location: region ? { region } : undefined,
    validated: true,
  };

  const { data, error, loading } = useSubscription(SUBSCRIBE_TO_CLIMATE_DATA, {
    variables: { filter },
    onSubscriptionData: ({ subscriptionData }) => {
      const newData = subscriptionData.data?.newClimateData;
      if (newData) {
        setClimateUpdates(prev => [newData, ...prev.slice(0, 99)]); // Keep last 100 updates

        // Check for alerts
        const alerts = checkForClimateAlerts(newData);
        if (alerts.length > 0) {
          setAlerts(prev => [...alerts, ...prev.slice(0, 49)]); // Keep last 50 alerts
        }
      }
    },
  });

  const checkForClimateAlerts = data => {
    const alerts = [];
    const { measurements } = data;

    // Temperature alerts
    if (measurements.temperature > 40 || measurements.temperature < -20) {
      alerts.push({
        type: 'extreme_temperature',
        severity: 'high',
        message: `Extreme temperature detected: ${measurements.temperature}°C`,
        data,
      });
    }

    // Wind speed alerts
    if (measurements.windSpeed > 25) {
      alerts.push({
        type: 'high_wind',
        severity: 'medium',
        message: `High wind speed detected: ${measurements.windSpeed} m/s`,
        data,
      });
    }

    // Air quality alerts
    if (measurements.airQuality?.aqi > 150) {
      alerts.push({
        type: 'poor_air_quality',
        severity: 'high',
        message: `Poor air quality detected: AQI ${measurements.airQuality.aqi}`,
        data,
      });
    }

    return alerts;
  };

  return {
    climateUpdates,
    alerts,
    loading,
    error,
    clearAlerts: () => setAlerts([]),
    clearUpdates: () => setClimateUpdates([]),
  };
}
```

---

## 4. Advanced Features and Optimizations

### Query Optimization and Caching

```javascript
// utils/query-optimization.js
import { DocumentNode } from 'graphql';
import { gql } from '@apollo/client';

class QueryOptimizer {
  constructor(client) {
    this.client = client;
    this.queryCache = new Map();
    this.fragmentCache = new Map();
  }

  // Fragment library for reusable query parts
  fragments = {
    ClimateDataBasic: gql`
      fragment ClimateDataBasic on ClimateData {
        id
        timestamp
        stationId
        submitter
        location {
          latitude
          longitude
        }
        measurements {
          temperature
          humidity
          pressure
        }
        validation {
          status
          accuracy
        }
      }
    `,

    ClimateDataDetailed: gql`
      fragment ClimateDataDetailed on ClimateData {
        ...ClimateDataBasic
        measurements {
          temperature
          humidity
          pressure
          windSpeed
          windDirection
          precipitation
          uvIndex
          airQuality {
            aqi
            pm25
            pm10
            o3
            no2
            so2
            co
          }
        }
        validation {
          status
          accuracy
          validatedAt
          validators {
            address
            stake
            reputation
          }
        }
        ipfsHash
        onChainTxHash
        rewards {
          amount
          distributed
          pending
        }
      }
    `,

    TransactionBasic: gql`
      fragment TransactionBasic on Transaction {
        hash
        from
        to
        value
        timestamp
        status
        gasUsed
      }
    `,

    WeatherStationBasic: gql`
      fragment WeatherStationBasic on WeatherStation {
        id
        name
        location {
          latitude
          longitude
          elevation
        }
        status
        deviceInfo {
          model
          manufacturer
        }
        totalSubmissions
        validationAccuracy
      }
    `,
  };

  // Optimize query based on use case
  optimizeQuery(baseQuery, options = {}) {
    const {
      fields = [],
      fragments = [],
      pagination = { limit: 20, offset: 0 },
      cachePolicy = 'cache-first',
    } = options;

    // Build optimized query
    let optimizedQuery = baseQuery;

    // Add fragments if specified
    if (fragments.length > 0) {
      fragments.forEach(fragmentName => {
        if (this.fragments[fragmentName]) {
          optimizedQuery = gql`
            ${this.fragments[fragmentName]}
            ${optimizedQuery}
          `;
        }
      });
    }

    return {
      query: optimizedQuery,
      fetchPolicy: cachePolicy,
      variables: {
        pagination,
        ...options.variables,
      },
    };
  }

  // Batch multiple queries efficiently
  async batchQueries(queries) {
    const promises = queries.map(({ query, variables, options = {} }) => {
      return this.client.query({
        query,
        variables,
        ...options,
      });
    });

    try {
      const results = await Promise.allSettled(promises);
      return results.map((result, index) => ({
        index,
        success: result.status === 'fulfilled',
        data: result.status === 'fulfilled' ? result.value.data : null,
        error: result.status === 'rejected' ? result.reason : null,
      }));
    } catch (error) {
      console.error('Batch query error:', error);
      throw error;
    }
  }

  // Prefetch data for performance
  async prefetchData(queries, priority = 'normal') {
    const prefetchPromises = queries.map(({ query, variables }) => {
      return this.client.query({
        query,
        variables,
        fetchPolicy: 'cache-first',
        errorPolicy: 'ignore',
      });
    });

    if (priority === 'high') {
      await Promise.all(prefetchPromises);
    } else {
      // Fire and forget for normal priority
      Promise.all(prefetchPromises).catch(console.warn);
    }
  }

  // Cache management
  getCacheStats() {
    const cache = this.client.cache;
    return {
      size: cache.extract ? Object.keys(cache.extract()).length : 0,
      queries: this.queryCache.size,
      fragments: this.fragmentCache.size,
    };
  }

  clearCache(pattern = null) {
    if (pattern) {
      // Clear specific cache entries
      this.client.cache.evict({ fieldName: pattern });
    } else {
      // Clear all cache
      this.client.cache.reset();
    }
  }
}

// Performance monitoring
class GraphQLPerformanceMonitor {
  constructor() {
    this.metrics = {
      queries: new Map(),
      subscriptions: new Map(),
      mutations: new Map(),
    };
  }

  startOperation(operationName, type = 'query') {
    const startTime = performance.now();
    return {
      operationName,
      type,
      startTime,
      end: () => {
        const duration = performance.now() - startTime;
        this.recordMetric(operationName, type, duration);
        return duration;
      },
    };
  }

  recordMetric(operationName, type, duration) {
    if (!this.metrics[type]) {
      this.metrics[type] = new Map();
    }

    const existing = this.metrics[type].get(operationName) || {
      count: 0,
      totalTime: 0,
      minTime: Infinity,
      maxTime: 0,
      avgTime: 0,
    };

    existing.count++;
    existing.totalTime += duration;
    existing.minTime = Math.min(existing.minTime, duration);
    existing.maxTime = Math.max(existing.maxTime, duration);
    existing.avgTime = existing.totalTime / existing.count;

    this.metrics[type].set(operationName, existing);
  }

  getMetrics() {
    const result = {};
    for (const [type, operations] of Object.entries(this.metrics)) {
      result[type] = {};
      for (const [operation, metrics] of operations) {
        result[type][operation] = metrics;
      }
    }
    return result;
  }

  getSlowQueries(threshold = 1000) {
    const slowQueries = [];
    for (const [type, operations] of Object.entries(this.metrics)) {
      for (const [operation, metrics] of operations) {
        if (metrics.avgTime > threshold) {
          slowQueries.push({
            operation,
            type,
            avgTime: metrics.avgTime,
            maxTime: metrics.maxTime,
            count: metrics.count,
          });
        }
      }
    }
    return slowQueries.sort((a, b) => b.avgTime - a.avgTime);
  }
}

export { QueryOptimizer, GraphQLPerformanceMonitor };
```

### Error Handling and Retry Logic

```javascript
// utils/error-handling.js
import { ApolloError } from '@apollo/client';

class GraphQLErrorHandler {
  constructor(config = {}) {
    this.config = {
      maxRetries: config.maxRetries || 3,
      retryDelay: config.retryDelay || 1000,
      retryMultiplier: config.retryMultiplier || 2,
      enableLogging: config.enableLogging !== false,
      ...config,
    };
  }

  async handleError(error, operation, variables = {}, attempt = 1) {
    this.logError(error, operation, attempt);

    if (error instanceof ApolloError) {
      return this.handleApolloError(error, operation, variables, attempt);
    } else if (error.networkError) {
      return this.handleNetworkError(error.networkError, operation, variables, attempt);
    } else {
      return this.handleUnknownError(error, operation, variables, attempt);
    }
  }

  async handleApolloError(error, operation, variables, attempt) {
    const { graphQLErrors, networkError } = error;

    // Handle GraphQL errors
    if (graphQLErrors && graphQLErrors.length > 0) {
      for (const gqlError of graphQLErrors) {
        const errorCode = gqlError.extensions?.code;

        switch (errorCode) {
          case 'UNAUTHENTICATED':
            throw new Error('Authentication failed. Please check your API credentials.');

          case 'FORBIDDEN':
            throw new Error('Access denied. Insufficient permissions.');

          case 'RATE_LIMITED':
            if (attempt <= this.config.maxRetries) {
              const delay = this.calculateDelay(attempt);
              console.warn(`Rate limited. Retrying in ${delay}ms...`);
              await this.delay(delay);
              return { shouldRetry: true, delay };
            }
            throw new Error('Rate limit exceeded. Please try again later.');

          case 'VALIDATION_ERROR':
            throw new Error(`Validation error: ${gqlError.message}`);

          case 'INTERNAL_ERROR':
            if (attempt <= this.config.maxRetries) {
              const delay = this.calculateDelay(attempt);
              console.warn(`Internal server error. Retrying in ${delay}ms...`);
              await this.delay(delay);
              return { shouldRetry: true, delay };
            }
            throw new Error('Internal server error. Please try again later.');

          default:
            console.error('Unhandled GraphQL error:', gqlError);
            throw new Error(gqlError.message || 'GraphQL error occurred');
        }
      }
    }

    // Handle network errors
    if (networkError) {
      return this.handleNetworkError(networkError, operation, variables, attempt);
    }

    throw error;
  }

  async handleNetworkError(networkError, operation, variables, attempt) {
    const { statusCode } = networkError;

    switch (statusCode) {
      case 408: // Request Timeout
      case 429: // Too Many Requests
      case 500: // Internal Server Error
      case 502: // Bad Gateway
      case 503: // Service Unavailable
      case 504: // Gateway Timeout
        if (attempt <= this.config.maxRetries) {
          const delay = this.calculateDelay(attempt);
          console.warn(`Network error ${statusCode}. Retrying in ${delay}ms...`);
          await this.delay(delay);
          return { shouldRetry: true, delay };
        }
        break;

      case 401: // Unauthorized
        throw new Error('Authentication failed. Please check your credentials.');

      case 403: // Forbidden
        throw new Error('Access denied. Insufficient permissions.');

      case 404: // Not Found
        throw new Error('Resource not found.');

      default:
        console.error('Network error:', networkError);
    }

    throw new Error(`Network error: ${networkError.message || statusCode}`);
  }

  async handleUnknownError(error, operation, variables, attempt) {
    console.error('Unknown error:', error);

    if (attempt <= this.config.maxRetries) {
      const delay = this.calculateDelay(attempt);
      console.warn(`Unknown error. Retrying in ${delay}ms...`);
      await this.delay(delay);
      return { shouldRetry: true, delay };
    }

    throw new Error(`Unknown error: ${error.message}`);
  }

  calculateDelay(attempt) {
    return this.config.retryDelay * Math.pow(this.config.retryMultiplier, attempt - 1);
  }

  delay(ms) {
    return new Promise(resolve => setTimeout(resolve, ms));
  }

  logError(error, operation, attempt) {
    if (!this.config.enableLogging) return;

    console.error(`GraphQL Error (attempt ${attempt}):`, {
      operation,
      error: error.message,
      stack: error.stack,
      timestamp: new Date().toISOString(),
    });
  }
}

// Retry wrapper for GraphQL operations
export function withRetry(client, errorHandler) {
  return {
    async query(query, variables = {}, options = {}) {
      let attempt = 1;
      const maxAttempts = options.maxRetries || errorHandler.config.maxRetries;

      while (attempt <= maxAttempts) {
        try {
          return await client.query({ query, variables, ...options });
        } catch (error) {
          const result = await errorHandler.handleError(error, 'query', variables, attempt);

          if (!result.shouldRetry || attempt === maxAttempts) {
            throw error;
          }

          attempt++;
        }
      }
    },

    async mutate(mutation, variables = {}, options = {}) {
      let attempt = 1;
      const maxAttempts = options.maxRetries || errorHandler.config.maxRetries;

      while (attempt <= maxAttempts) {
        try {
          return await client.mutate({ mutation, variables, ...options });
        } catch (error) {
          const result = await errorHandler.handleError(error, 'mutation', variables, attempt);

          if (!result.shouldRetry || attempt === maxAttempts) {
            throw error;
          }

          attempt++;
        }
      }
    },
  };
}

export default GraphQLErrorHandler;
```

---

## Next Steps

1. **Build [End-to-End Applications](../applications/)**
2. **Create [Real-time Dashboards](../applications/02-realtime-dashboard.md)**
3. **Implement [IoT Integration Examples](../iot-integration/)**

---

## GraphQL Endpoint Reference

### Base URLs

- **Mainnet**: `https://graphql.wrfcoin.network/graphql`
- **Testnet**: `https://graphql.testnet.wrfcoin.network/graphql`

### WebSocket URLs

- **Mainnet**: `wss://graphql.wrfcoin.network/graphql`
- **Testnet**: `wss://graphql.testnet.wrfcoin.network/graphql`

### Authentication

- **Method**: Bearer Token + HMAC Signature
- **Headers**: `Authorization`, `X-API-Key`, `X-Timestamp`, `X-Signature`

This comprehensive GraphQL integration guide provides advanced patterns for building efficient,
real-time applications with the WRFCoin platform.
