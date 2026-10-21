/**
 * Comprehensive Fuzz Testing Configuration for Wrfcoin Platform
 */

module.exports = {
  // API Server Endpoints Configuration
  apiServers: {
    main: {
      baseUrl: 'http://localhost:4001',
      name: 'Main API Server',
      endpoints: [
        { method: 'GET', path: '/health' },
        { method: 'POST', path: '/api/transactions/create' },
        { method: 'GET', path: '/api/blockchain/stats' },
        { method: 'POST', path: '/api/governance/proposal' },
        { method: 'POST', path: '/api/nft/create' },
      ],
    },
    wallet: {
      baseUrl: 'http://localhost:4000',
      name: 'Wallet Server',
      endpoints: [
        { method: 'POST', path: '/api/wallet/generate-mnemonic' },
        { method: 'POST', path: '/api/wallet/validate-mnemonic' },
        { method: 'POST', path: '/api/wallet/create' },
        { method: 'POST', path: '/api/wallet/transfer' },
      ],
    },
    crossChain: {
      baseUrl: 'http://localhost:3000',
      name: 'Cross-Chain Protocol Server',
      endpoints: [
        { method: 'POST', path: '/api/bridge/transfer' },
        { method: 'GET', path: '/api/bridge/status' },
        { method: 'POST', path: '/api/bridge/claim' },
      ],
    },
    compute: {
      baseUrl: 'http://localhost:6000',
      name: 'Compute Networks Server',
      endpoints: [
        { method: 'POST', path: '/api/compute/submit-job' },
        { method: 'GET', path: '/api/compute/status' },
      ],
    },
    storage: {
      baseUrl: 'http://localhost:7000',
      name: 'Storage Networks Server',
      endpoints: [
        { method: 'POST', path: '/api/storage/upload' },
        { method: 'GET', path: '/api/storage/retrieve' },
      ],
    },
  },

  // WebSocket Configuration
  websocket: {
    url: 'ws://localhost:4002/ws',
    channels: [
      'global',
      'blockchain',
      'computational',
      'cross-chain',
      'governance',
      'mining',
    ],
    messageTypes: [
      'subscribe',
      'unsubscribe',
      'ping',
      'transaction_broadcast',
      'weather_data_submission',
    ],
  },

  // Critical Components for Internal Fuzzing
  criticalComponents: [
    'wallet operations',
    'transaction processing',
    'consensus mechanisms',
    'weather data validation',
    'cross-chain bridge operations',
    'NFT creation and marketplace',
    'governance proposal system',
  ],

  // Fuzzing Parameters
  fuzzing: {
    duration: {
      quick: 60000, // 1 minute
      standard: 300000, // 5 minutes
      extended: 1800000, // 30 minutes
      continuous: -1, // Run indefinitely
    },
    requestsPerSecond: {
      low: 1,
      medium: 5,
      high: 10,
      stress: 50,
    },
    maxPayloadSize: 1024 * 1024, // 1MB
    timeouts: {
      request: 30000, // 30 seconds
      websocket: 5000, // 5 seconds
    },
  },

  // Property-Based Testing Configuration
  propertyBased: {
    numRuns: 1000,
    maxSize: 100,
    timeout: 30000,
    verbose: true,
  },

  // Security Testing Parameters
  security: {
    sqlInjectionPatterns: [
      "'; DROP TABLE users; --",
      "' OR '1'='1",
      "'; UNION SELECT * FROM users; --",
      "admin'--",
      "' OR 1=1--",
    ],
    xssPatterns: [
      "<script>alert('XSS')</script>",
      "javascript:alert('XSS')",
      "<img src='x' onerror='alert(1)'>",
      '<svg onload=alert(1)>',
    ],
    bufferOverflowPatterns: [
      'A'.repeat(10000),
      'A'.repeat(100000),
      Buffer.alloc(65536, 'A').toString(),
    ],
  },

  // Reporting Configuration
  reporting: {
    outputDir: './reports',
    formats: ['json', 'html', 'csv'],
    includeRequestBodies: true,
    includeResponseBodies: false,
    maxLogSize: 10 * 1024 * 1024, // 10MB
  },

  // Schema Definitions for REST API Fuzzing
  schemas: {
    transaction: {
      type: 'object',
      required: ['from', 'to', 'amount'],
      properties: {
        from: { type: 'string', pattern: '^Wrf[0-9A-Za-z]{32}$' },
        to: { type: 'string', pattern: '^Wrf[0-9A-Za-z]{32}$' },
        amount: { type: 'number', minimum: 0, maximum: 1000000 },
        fee: { type: 'number', minimum: 0, maximum: 100 },
        data: { type: 'string', maxLength: 1024 },
      },
    },
    wallet: {
      type: 'object',
      required: ['strength'],
      properties: {
        strength: { type: 'number', enum: [128, 160, 192, 224, 256] },
        sessionId: { type: 'string', maxLength: 64 },
      },
    },
    weatherData: {
      type: 'object',
      required: ['stationId', 'temperature', 'timestamp'],
      properties: {
        stationId: { type: 'string', maxLength: 32 },
        temperature: { type: 'number', minimum: -50, maximum: 60 },
        humidity: { type: 'number', minimum: 0, maximum: 100 },
        pressure: { type: 'number', minimum: 800, maximum: 1200 },
        timestamp: { type: 'string', format: 'date-time' },
      },
    },
    bridgeTransfer: {
      type: 'object',
      required: ['fromChain', 'toChain', 'amount', 'recipientAddress'],
      properties: {
        fromChain: {
          type: 'string',
          enum: ['wrfcoin', 'ethereum', 'polygon', 'solana'],
        },
        toChain: {
          type: 'string',
          enum: ['wrfcoin', 'ethereum', 'polygon', 'solana'],
        },
        amount: { type: 'number', minimum: 0.001, maximum: 100000 },
        recipientAddress: { type: 'string', minLength: 20, maxLength: 64 },
      },
    },
  },
};
