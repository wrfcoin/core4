// E2E Test Configuration
const config = {
  // Base URLs for different environments
  baseUrls: {
    frontend: process.env.E2E_FRONTEND_URL || 'http://localhost:8080',
    backend: process.env.E2E_BACKEND_URL || 'http://localhost:8080',
    graphql: process.env.E2E_GRAPHQL_URL || 'http://localhost:8080',
    blockchain: process.env.E2E_BLOCKCHAIN_URL || 'http://localhost:8545',
  },

  // Test timeouts
  timeouts: {
    navigation: 30000,
    api: 15000,
    blockchain: 60000,
    fileUpload: 30000,
    modal: 5000,
  },

  // Test data paths
  testData: {
    users: './fixtures/mock-users.json',
    sensors: './fixtures/mock-sensors.json',
    nfts: './fixtures/mock-nfts.json',
    transactions: './fixtures/mock-transactions.json',
  },

  // Browser configuration
  browser: {
    headless: process.env.CI === 'true',
    viewport: { width: 1280, height: 720 },
    deviceScaleFactor: 1,
    hasTouch: false,
    isMobile: false,
  },

  // Test environment settings
  environment: {
    name: process.env.E2E_ENV || 'local',
    cleanup: process.env.E2E_CLEANUP !== 'false',
    screenshots: process.env.E2E_SCREENSHOTS !== 'false',
    videos: process.env.E2E_VIDEOS !== 'false',
  },

  // Blockchain test configuration
  blockchain: {
    network: 'hardhat',
    chainId: 31337,
    accounts: {
      deployer:
        '0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80',
      user1:
        '0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d',
      user2:
        '0x5de4111afa1a4b94908f83103eb1f1706367c2e68ca870fc3fb9a804cdab365a',
    },
    tokens: {
      wrf: '0x5FbDB2315678afecb367f032d93F642f64180aa3',
      usdc: '0xe7f1725E7734CE288F8367e1Bb143E90bb3F0512',
    },
  },

  // API configuration
  api: {
    retries: 3,
    retryDelay: 1000,
    headers: {
      'Content-Type': 'application/json',
      Accept: 'application/json',
    },
  },

  // Test selectors
  selectors: {
    // Navigation
    navigation: {
      dashboard: '[data-testid="nav-dashboard"]',
      wallet: '[data-testid="nav-wallet"]',
      nft: '[data-testid="nav-nft"]',
      governance: '[data-testid="nav-governance"]',
      bridge: '[data-testid="nav-bridge"]',
    },

    // Common components
    buttons: {
      primary: '[data-testid="btn-primary"]',
      secondary: '[data-testid="btn-secondary"]',
      submit: '[data-testid="btn-submit"]',
      cancel: '[data-testid="btn-cancel"]',
      close: '[data-testid="btn-close"]',
    },

    // Forms
    forms: {
      input: '[data-testid="form-input"]',
      select: '[data-testid="form-select"]',
      checkbox: '[data-testid="form-checkbox"]',
      radio: '[data-testid="form-radio"]',
      textarea: '[data-testid="form-textarea"]',
    },

    // Modals and dialogs
    modals: {
      container: '[data-testid="modal"]',
      header: '[data-testid="modal-header"]',
      body: '[data-testid="modal-body"]',
      footer: '[data-testid="modal-footer"]',
    },

    // Loading states
    loading: {
      spinner: '[data-testid="loading-spinner"]',
      skeleton: '[data-testid="loading-skeleton"]',
      progress: '[data-testid="loading-progress"]',
    },

    // Notifications
    notifications: {
      success: '[data-testid="notification-success"]',
      error: '[data-testid="notification-error"]',
      warning: '[data-testid="notification-warning"]',
      info: '[data-testid="notification-info"]',
    },
  },

  // Performance thresholds
  performance: {
    pageLoad: 3000,
    apiResponse: 1000,
    blockchainTransaction: 30000,
    fileUpload: 10000,
  },

  // Test data generation
  dataGeneration: {
    users: {
      count: 10,
      roles: ['user', 'admin', 'validator', 'provider'],
    },
    sensors: {
      count: 50,
      types: ['temperature', 'humidity', 'pressure', 'wind'],
    },
    nfts: {
      count: 20,
      categories: ['computational', 'weather', 'carbon-credit'],
    },
  },
};

// Environment-specific overrides
const envOverrides = {
  staging: {
    baseUrls: {
      frontend: 'https://staging.wrfcoin.com',
      backend: 'https://api-staging.wrfcoin.com',
      graphql: 'https://graphql-staging.wrfcoin.com',
    },
    timeouts: {
      navigation: 45000,
      api: 20000,
      blockchain: 90000,
    },
  },
  production: {
    baseUrls: {
      frontend: 'https://app.wrfcoin.com',
      backend: 'https://api.wrfcoin.com',
      graphql: 'https://graphql.wrfcoin.com',
    },
    timeouts: {
      navigation: 60000,
      api: 30000,
      blockchain: 120000,
    },
    environment: {
      cleanup: false,
      screenshots: true,
      videos: true,
    },
  },
};

// Apply environment-specific overrides
if (envOverrides[config.environment.name]) {
  Object.assign(config, envOverrides[config.environment.name]);
}

module.exports = config;
