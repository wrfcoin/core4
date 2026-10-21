/**
 * Pact Testing Setup Configuration
 * Sets up the Pact testing environment for consumer-provider contract testing
 */

const { Pact } = require('@pact-foundation/pact');
const { Matchers } = require('@pact-foundation/pact');
const path = require('path');

// Fail-fast: require PACT_BROKER_PASSWORD from environment
if (!process.env.PACT_BROKER_PASSWORD) {
  throw new Error(
    'PACT_BROKER_PASSWORD must be set. See .env.example for instructions.'
  );
}

// Pact configuration constants
const PACT_CONFIG = {
  consumer: 'wrfcoin-api',
  dir: path.resolve(process.cwd(), 'tests/contract/pacts'),
  log: path.resolve(process.cwd(), 'tests/contract/logs/pact.log'),
  logLevel: process.env.PACT_LOG_LEVEL || 'info',
  spec: 3, // Pact specification version
  timeout: 30000,
  cors: true,
  host: 'localhost',
  // Enhanced configuration for better reliability
  pactfileWriteMode: 'merge', // Merge multiple interactions into single pact file
  format: 'json',
  ssl: false,
  stateHandler: {
    // Global state handlers for common states
    'database is empty': async () => {
      console.log('Setting up empty database state');
    },
    'blockchain is operational': async () => {
      console.log('Setting up operational blockchain state');
    },
    'weather service is operational': async () => {
      console.log('Setting up operational weather service state');
    },
  },
  // Enhanced broker configuration
  broker: {
    brokerUrl: process.env.PACT_BROKER_URL || 'http://localhost:9292',
    username: process.env.PACT_BROKER_USERNAME || 'admin',
    password: process.env.PACT_BROKER_PASSWORD,
    publishVerificationResult: process.env.CI === 'true',
    providerVersion: process.env.GIT_SHA || '1.0.0',
    consumerVersion: process.env.GIT_SHA || '1.0.0',
    tags: process.env.PACT_CONSUMER_TAG
      ? [process.env.PACT_CONSUMER_TAG]
      : ['dev'],
  },
  // Retry configuration for flaky tests
  retry: {
    maxAttempts: 3,
    delay: 1000,
  },
};

// Provider configurations
const PROVIDERS = {
  postgres: {
    name: 'wrfcoin-postgres-db',
    port: 5434, // Test database port
  },
  blockchain: {
    name: 'wrfcoin-blockchain-rpc',
    port: 8545, // Mock blockchain RPC port
  },
  weather: {
    name: 'wrfcoin-weather-api',
    port: 3010, // Mock weather API port
  },
  carbon: {
    name: 'wrfcoin-carbon-oracle',
    port: 3020, // Mock carbon oracle port
  },
};

// Create Pact provider instances
const createPactProvider = (providerName, port) => {
  const pactInstance = new Pact({
    consumer: PACT_CONFIG.consumer,
    provider: providerName,
    port: port,
    dir: PACT_CONFIG.dir,
    log: PACT_CONFIG.log,
    logLevel: PACT_CONFIG.logLevel,
    spec: PACT_CONFIG.spec,
    timeout: PACT_CONFIG.timeout,
    cors: PACT_CONFIG.cors,
    host: PACT_CONFIG.host,
    pactfileWriteMode: PACT_CONFIG.pactfileWriteMode,
    format: PACT_CONFIG.format,
    ssl: PACT_CONFIG.ssl,
  });

  // Enhanced provider with additional methods
  return {
    ...pactInstance,
    providerName,
    port,

    // Enhanced setup with retry logic
    async setup() {
      let attempts = 0;
      while (attempts < PACT_CONFIG.retry.maxAttempts) {
        try {
          await pactInstance.setup();
          console.log(
            `✅ Pact provider ${providerName} setup successful on port ${port}`
          );
          return;
        } catch (error) {
          attempts++;
          console.warn(
            `⚠️ Pact provider ${providerName} setup attempt ${attempts} failed:`,
            error.message
          );
          if (attempts >= PACT_CONFIG.retry.maxAttempts) {
            throw new Error(
              `Failed to setup Pact provider ${providerName} after ${attempts} attempts: ${error.message}`
            );
          }
          await new Promise(resolve =>
            setTimeout(resolve, PACT_CONFIG.retry.delay)
          );
        }
      }
    },

    // Enhanced finalize with verification
    async finalize() {
      try {
        await pactInstance.finalize();
        console.log(`✅ Pact provider ${providerName} finalized successfully`);
      } catch (error) {
        console.error(
          `❌ Failed to finalize Pact provider ${providerName}:`,
          error.message
        );
        throw error;
      }
    },

    // Enhanced verify with custom state handlers
    async verify() {
      try {
        const result = await pactInstance.verify();
        console.log(`✅ Pact verification completed for ${providerName}`);
        return result;
      } catch (error) {
        console.error(
          `❌ Pact verification failed for ${providerName}:`,
          error.message
        );
        throw error;
      }
    },

    // Method to add custom state handlers
    addStateHandler(state, handler) {
      if (!this.stateHandlers) {
        this.stateHandlers = {};
      }
      this.stateHandlers[state] = handler;
    },

    // Method to publish to Pact Broker with enhanced versioning
    async publishToBroker() {
      if (!PACT_CONFIG.broker.brokerUrl) {
        console.log(
          `ℹ️ No Pact Broker URL configured, skipping publish for ${providerName}`
        );
        return;
      }

      try {
        console.log(`📤 Publishing Pact for ${providerName} to broker...`);

        // Use the enhanced publishing script for proper version management
        const { ContractPublisher } = require('../scripts/publish-pacts.js');
        const publisher = new ContractPublisher({
          providerName: providerName,
          specificPactFile: path.resolve(
            PACT_CONFIG.dir,
            `${PACT_CONFIG.consumer}-${providerName}.json`
          ),
        });

        await publisher.publishContracts();
        console.log(
          `✅ Published Pact for ${providerName} to ${PACT_CONFIG.broker.brokerUrl}`
        );
      } catch (error) {
        console.error(
          `❌ Failed to publish Pact for ${providerName}:`,
          error.message
        );
        throw error;
      }
    },
  };
};

// Initialize Pact providers
const pactProviders = {
  postgres: createPactProvider(
    PROVIDERS.postgres.name,
    PROVIDERS.postgres.port
  ),
  blockchain: createPactProvider(
    PROVIDERS.blockchain.name,
    PROVIDERS.blockchain.port
  ),
  weather: createPactProvider(PROVIDERS.weather.name, PROVIDERS.weather.port),
  carbon: createPactProvider(PROVIDERS.carbon.name, PROVIDERS.carbon.port),
};

// Pact matchers for flexible contract testing
const { like, eachLike, term, uuid, iso8601DateTime } = Matchers;

const commonMatchers = {
  // Basic matchers
  id: like(1),
  uuid: uuid(),
  timestamp: iso8601DateTime(),

  // Communication matchers
  email: term({
    generate: 'test@wrfcoin.com',
    matcher: '^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$',
  }),

  // Blockchain matchers
  walletAddress: term({
    generate: '0x742d35Cc6634C0532925a3b8D8aE7b8e4B9f1234',
    matcher: '^0x[a-fA-F0-9]{40}$',
  }),
  transactionHash: term({
    generate:
      '0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890',
    matcher: '^0x[a-fA-F0-9]{64}$',
  }),
  blockHash: term({
    generate:
      '0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890',
    matcher: '^0x[a-fA-F0-9]{64}$',
  }),
  contractAddress: term({
    generate: '0x742d35Cc6634C0532925a3b8D8aE7b8e4B9f1234',
    matcher: '^0x[a-fA-F0-9]{40}$',
  }),
  hexValue: term({
    generate: '0x1a2b3c',
    matcher: '^0x[a-fA-F0-9]+$',
  }),

  // Enhanced carbon measurement matcher
  carbonMeasurement: like({
    co2Equivalent: 1.234,
    calculationMethod: 'weather_monitoring_ipcc_2021',
    uncertainty: 5,
    timestamp: iso8601DateTime(),
    verificationStandard: 'vm0012',
    calculationId: uuid(),
    dataQuality: 'high',
  }),

  // Enhanced weather measurement matcher
  weatherMeasurement: like({
    temperature: 22.5,
    humidity: 65.0,
    pressure: 1013.25,
    windSpeed: 3.2,
    windDirection: 180,
    dewPoint: 15.8,
    visibility: 10.0,
    uvIndex: 5,
  }),

  // Enhanced user matchers
  userProfile: like({
    id: like(1),
    username: like('testuser'),
    email: term({
      generate: 'test@wrfcoin.com',
      matcher: '^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$',
    }),
    walletAddress: term({
      generate: '0x742d35Cc6634C0532925a3b8D8aE7b8e4B9f1234',
      matcher: '^0x[a-fA-F0-9]{40}$',
    }),
    status: like('active'),
    role: like('user'),
    reputation: like(0),
    isVerified: like(false),
    createdAt: iso8601DateTime(),
    updatedAt: iso8601DateTime(),
  }),

  // Transaction matchers
  transactionDetail: like({
    id: like(1),
    txId: term({
      generate:
        '0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890',
      matcher: '^0x[a-fA-F0-9]{64}$',
    }),
    fromAddress: term({
      generate: '0x742d35Cc6634C0532925a3b8D8aE7b8e4B9f1234',
      matcher: '^0x[a-fA-F0-9]{40}$',
    }),
    toAddress: term({
      generate: '0x856f48C9247d7b8e3D4B8F5e2C9A7b8E4C5F1234',
      matcher: '^0x[a-fA-F0-9]{40}$',
    }),
    amount: like(1000000000),
    fee: like(21000),
    status: like('confirmed'),
    type: like('transfer'),
    timestamp: iso8601DateTime(),
    blockNumber: like(1000000),
    confirmations: like(12),
  }),

  // Weather station matchers
  weatherStation: like({
    id: like('station-1'),
    name: like('Test Weather Station'),
    location: like({
      lat: 37.7749,
      lng: -122.4194,
      elevation: 52,
      address: like('San Francisco, CA'),
      timezone: like('America/Los_Angeles'),
    }),
    isActive: like(true),
    lastSeen: iso8601DateTime(),
    firmware: like('v2.1.0'),
    hardware: like('WRF-STATION-PRO'),
  }),

  // API response wrappers
  successResponse: data => ({
    success: true,
    data: data,
    timestamp: iso8601DateTime(),
  }),

  errorResponse: (message = 'An error occurred') => ({
    success: false,
    error: like(message),
    timestamp: iso8601DateTime(),
    errorCode: like('ERR_001'),
  }),

  paginatedResponse: (data, total = null) => ({
    success: true,
    data: data,
    pagination: like({
      total: total || like(100),
      limit: like(10),
      offset: like(0),
      hasMore: like(false),
    }),
    timestamp: iso8601DateTime(),
  }),

  // Validation error patterns
  validationError: like({
    success: false,
    error: like('Validation failed'),
    details: eachLike(
      {
        field: like('username'),
        message: like('Username is required'),
        code: like('REQUIRED_FIELD'),
      },
      { min: 1 }
    ),
    timestamp: iso8601DateTime(),
  }),

  // Performance and monitoring matchers
  healthCheck: like({
    status: like('healthy'),
    service: like('wrfcoin-api'),
    version: like('1.0.0'),
    timestamp: iso8601DateTime(),
    uptime: like(3600),
    dependencies: eachLike(
      {
        name: like('database'),
        status: like('healthy'),
        responseTime: like(50),
      },
      { min: 1 }
    ),
  }),
};

// Setup helper functions
const setupPactProvider = async providerName => {
  const provider = pactProviders[providerName];
  if (!provider) {
    throw new Error(`Unknown provider: ${providerName}`);
  }

  console.log(`Setting up Pact provider: ${providerName}`);
  await provider.setup();
  return provider;
};

const teardownPactProvider = async providerName => {
  const provider = pactProviders[providerName];
  if (!provider) {
    throw new Error(`Unknown provider: ${providerName}`);
  }

  console.log(`Tearing down Pact provider: ${providerName}`);
  await provider.finalize();
};

const setupAllProviders = async () => {
  console.log('Setting up all Pact providers...');
  const setupPromises = Object.keys(pactProviders).map(setupPactProvider);
  await Promise.all(setupPromises);
  console.log('All Pact providers set up successfully');
};

const teardownAllProviders = async () => {
  console.log('Tearing down all Pact providers...');
  const teardownPromises = Object.keys(pactProviders).map(teardownPactProvider);
  await Promise.all(teardownPromises);
  console.log('All Pact providers torn down successfully');
};

// Error handling utilities
const handlePactError = (error, context) => {
  console.error(`Pact error in ${context}:`, error);
  if (error.message && error.message.includes('EADDRINUSE')) {
    throw new Error(
      `Port already in use. Please ensure no other services are running on the required ports.`
    );
  }
  throw error;
};

// Verification utilities
const verifyPactContracts = async (provider, options = {}) => {
  const { Verifier } = require('@pact-foundation/pact');

  const defaultOptions = {
    provider: provider,
    providerBaseUrl: `http://localhost:${PROVIDERS[provider]?.port || 3000}`,
    pactUrls: [
      path.resolve(
        PACT_CONFIG.dir,
        `${PACT_CONFIG.consumer}-${PROVIDERS[provider]?.name}.json`
      ),
    ],
    publishVerificationResult: process.env.CI === 'true',
    providerVersion: process.env.npm_package_version || '1.0.0',
    timeout: 30000,
  };

  const verifier = new Verifier({
    ...defaultOptions,
    ...options,
  });

  return await verifier.verifyProvider();
};

// Enhanced contract testing utilities
const contractTestUtils = {
  // Helper to create state-dependent interactions
  createStatefulInteraction: (provider, state, interaction) => {
    return provider
      .given(state)
      .uponReceiving(interaction.description)
      .withRequest(interaction.request)
      .willRespondWith(interaction.response);
  },

  // Helper to test error scenarios
  createErrorScenario: (provider, scenario) => {
    return provider
      .given(scenario.state || 'error condition exists')
      .uponReceiving(scenario.description)
      .withRequest(scenario.request)
      .willRespondWith({
        status: scenario.errorCode || 400,
        headers: { 'Content-Type': 'application/json' },
        body: commonMatchers.errorResponse(scenario.errorMessage),
      });
  },

  // Helper to test pagination scenarios
  createPaginationScenario: (provider, config) => {
    const { state, description, path, query, data, total } = config;
    return provider
      .given(state || 'paginated data exists')
      .uponReceiving(description || 'a request for paginated data')
      .withRequest({
        method: 'GET',
        path: path,
        query: query || { limit: '10', offset: '0' },
      })
      .willRespondWith({
        status: 200,
        headers: { 'Content-Type': 'application/json' },
        body: commonMatchers.paginatedResponse(data, total),
      });
  },

  // Helper to test validation scenarios
  createValidationScenario: (provider, config) => {
    const { state, description, request, validationErrors } = config;
    return provider
      .given(state || 'validation is enforced')
      .uponReceiving(description || 'a request with invalid data')
      .withRequest(request)
      .willRespondWith({
        status: 422,
        headers: { 'Content-Type': 'application/json' },
        body: {
          ...commonMatchers.validationError,
          details:
            validationErrors ||
            eachLike(
              {
                field: like('field_name'),
                message: like('Field is required'),
                code: like('VALIDATION_ERROR'),
              },
              { min: 1 }
            ),
        },
      });
  },

  // Helper to test rate limiting scenarios
  createRateLimitScenario: (provider, config) => {
    const { state, description, request } = config;
    return provider
      .given(state || 'rate limit is enforced')
      .uponReceiving(description || 'too many requests')
      .withRequest(request)
      .willRespondWith({
        status: 429,
        headers: {
          'Content-Type': 'application/json',
          'Retry-After': like('60'),
          'X-Rate-Limit-Remaining': like('0'),
        },
        body: commonMatchers.errorResponse('Rate limit exceeded'),
      });
  },
};

// Enhanced error handling with detailed logging
const enhancedHandlePactError = async (error, context = {}) => {
  const errorDetails = {
    message: error.message,
    stack: error.stack,
    context: context,
    timestamp: new Date().toISOString(),
    provider: context.provider || 'unknown',
    testCase: context.testCase || 'unknown',
  };

  console.error(
    '🔥 Enhanced Pact Error Handler:',
    JSON.stringify(errorDetails, null, 2)
  );
  throw error;
};

module.exports = {
  Pact,
  Matchers,
  // Core functions
  createPactProvider,
  setupPactProvider,
  teardownPactProvider,
  setupAllProviders,
  teardownAllProviders,
  verifyPactContracts,
  handlePactError,
  // Enhanced utilities
  contractTestUtils,
  enhancedHandlePactError,
  // Configuration and data
  PACT_CONFIG,
  PROVIDERS,
  pactProviders,
  commonMatchers,
  // Matchers
  like,
  term,
  eachLike,
  uuid,
  iso8601DateTime,
};
