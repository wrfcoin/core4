// WRFCoin E2E Test Constants

export const TEST_CONSTANTS = {
  // API Endpoints
  API_ENDPOINTS: {
    MAIN_API: process.env.MAIN_API_URL || 'http://localhost:4001',
    CROSS_CHAIN: process.env.CROSS_CHAIN_URL || 'http://localhost:3000',
    WALLET_SERVER: process.env.WALLET_SERVER_URL || 'http://localhost:4000',
    COMPUTE_NETWORKS:
      process.env.COMPUTE_NETWORKS_URL || 'http://localhost:6000',
    IOT_SENSOR_NETWORKS:
      process.env.IOT_SENSOR_NETWORKS_URL || 'http://localhost:3002',
    GOVERNANCE_SERVER:
      process.env.GOVERNANCE_SERVER_URL || 'http://localhost:8000',
  },

  // Test Timeouts (in milliseconds)
  TIMEOUTS: {
    PAGE_LOAD: 30000,
    API_RESPONSE: 15000,
    TRANSACTION_CONFIRMATION: 60000,
    WALLET_CONNECTION: 30000,
    SMART_CONTRACT_INTERACTION: 45000,
  },

  // Test User Data
  TEST_USERS: {
    ADMIN: {
      email: 'admin@test.wrfcoin.com',
      password: 'AdminPassword123!',
      role: 'admin',
    },
    REGULAR_USER: {
      email: 'user@test.wrfcoin.com',
      password: 'UserPassword123!',
      role: 'user',
    },
    VALIDATOR: {
      email: 'validator@test.wrfcoin.com',
      password: 'ValidatorPassword123!',
      role: 'validator',
    },
  },

  // Wallet Addresses (Test)
  TEST_WALLETS: {
    PRIMARY: '0x742d35Cc6634C0532925a3b8D1c9e37C2384C24D',
    SECONDARY: '0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045',
    MULTISIG: '0x8ba1f109551bD432803012645Hac136c2ad2b6f5',
  },

  // Token Information
  TEST_TOKENS: {
    WRF: {
      symbol: 'WRF',
      decimals: 18,
      address: '0x1234567890123456789012345678901234567890',
    },
    CARBON: {
      symbol: 'CARBON',
      decimals: 18,
      address: '0x2345678901234567890123456789012345678901',
    },
    WEATHER: {
      symbol: 'WEATHER',
      decimals: 18,
      address: '0x3456789012345678901234567890123456789012',
    },
  },

  // Weather Data
  MOCK_WEATHER_DATA: {
    TEMPERATURE: 25.5,
    HUMIDITY: 65,
    PRESSURE: 1013.25,
    WIND_SPEED: 15.2,
    LOCATION: {
      LAT: 40.7128,
      LON: -74.006,
      CITY: 'New York',
    },
  },

  // NFT Test Data
  TEST_NFT_METADATA: {
    name: 'Test Weather NFT',
    description: 'A test NFT representing weather data',
    image: 'https://picsum.photos/400/400',
    attributes: [
      { trait_type: 'Temperature', value: '25.5°C' },
      { trait_type: 'Location', value: 'New York' },
      { trait_type: 'Rarity', value: 'Common' },
    ],
  },

  // DeFi Parameters
  DEFI_TEST_VALUES: {
    LIQUIDITY_AMOUNT: '1000',
    STAKE_AMOUNT: '500',
    STAKE_DURATION: 30, // days
    SLIPPAGE_TOLERANCE: 0.5, // percentage
  },

  // Governance Test Data
  GOVERNANCE_TEST_DATA: {
    PROPOSAL_TITLE: 'Test Governance Proposal',
    PROPOSAL_DESCRIPTION: 'This is a test proposal for E2E testing',
    VOTING_DURATION: 7, // days
    EXECUTION_DELAY: 1, // days
  },

  // Carbon Credit Data
  CARBON_CREDIT_DATA: {
    PROJECT_NAME: 'Test Carbon Project',
    CREDIT_AMOUNT: 100,
    PRICE_PER_CREDIT: 25.5,
    PROJECT_LOCATION: 'Test Forest Reserve',
    VINTAGE_YEAR: new Date().getFullYear(),
  },

  // Cross-Chain Bridge Data
  BRIDGE_TEST_DATA: {
    SOURCE_CHAIN: 'ethereum',
    DESTINATION_CHAIN: 'polygon',
    BRIDGE_AMOUNT: '10',
    BRIDGE_TOKEN: 'WRF',
  },

  // IoT Device Test Data
  IOT_DEVICE_DATA: {
    DEVICE_ID: 'TEST_DEVICE_001',
    DEVICE_TYPE: 'weather_station',
    LOCATION: 'Test Location',
    STATUS: 'active',
    LAST_READING: new Date().toISOString(),
  },

  // Test Network Configuration
  NETWORK_CONFIG: {
    CHAIN_ID: 31337,
    RPC_URL: 'http://localhost:8545',
    BLOCK_EXPLORER: 'http://localhost:8545',
    CURRENCY: {
      name: 'Test Ether',
      symbol: 'ETH',
      decimals: 18,
    },
  },

  // Performance Thresholds
  PERFORMANCE_THRESHOLDS: {
    PAGE_LOAD_TIME: 3000, // ms
    API_RESPONSE_TIME: 1000, // ms
    TRANSACTION_TIME: 30000, // ms
  },

  // Error Messages
  ERROR_MESSAGES: {
    WALLET_NOT_CONNECTED: 'Please connect your wallet',
    INSUFFICIENT_BALANCE: 'Insufficient balance',
    TRANSACTION_FAILED: 'Transaction failed',
    NETWORK_ERROR: 'Network error occurred',
    INVALID_ADDRESS: 'Invalid wallet address',
    UNAUTHORIZED: 'Unauthorized access',
  },

  // Success Messages
  SUCCESS_MESSAGES: {
    WALLET_CONNECTED: 'Wallet connected successfully',
    TRANSACTION_CONFIRMED: 'Transaction confirmed',
    NFT_MINTED: 'NFT minted successfully',
    PROPOSAL_CREATED: 'Proposal created successfully',
    VOTE_SUBMITTED: 'Vote submitted successfully',
  },

  // Selectors (Data Test IDs)
  SELECTORS: {
    // Wallet
    CONNECT_WALLET_BUTTON: '[data-testid="connect-wallet-button"]',
    WALLET_CONNECTED: '[data-testid="wallet-connected"]',
    WALLET_ADDRESS: '[data-testid="wallet-address"]',
    WALLET_BALANCE: '[data-testid="wallet-balance"]',

    // Navigation
    NAV_WALLET: '[data-testid="nav-wallet"]',
    NAV_NFT_MARKETPLACE: '[data-testid="nav-nft-marketplace"]',
    NAV_DEFI: '[data-testid="nav-defi"]',
    NAV_GOVERNANCE: '[data-testid="nav-governance"]',
    NAV_CARBON_CREDITS: '[data-testid="nav-carbon-credits"]',

    // Common
    LOADING_SPINNER: '[data-testid="loading-spinner"]',
    ERROR_MESSAGE: '[data-testid="error-message"]',
    SUCCESS_MESSAGE: '[data-testid="success-message"]',
    PAGE_LOADED: '[data-testid="page-loaded"]',
  },
};

export default TEST_CONSTANTS;
