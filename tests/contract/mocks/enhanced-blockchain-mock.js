const express = require('express');
const cors = require('cors');
const winston = require('winston');
const fs = require('fs');
const path = require('path');

/**
 * Enhanced Blockchain RPC Mock with State Management
 *
 * This enhanced version supports:
 * - Full JSON-RPC 2.0 protocol compliance
 * - Realistic blockchain state simulation
 * - Transaction pool and block mining simulation
 * - Smart contract deployment and interaction
 * - Multiple test scenarios and failure modes
 * - Comprehensive logging and metrics
 */

const app = express();
const PORT = process.env.PORT || 8545;

// Configuration from environment variables
const config = {
  scenario: process.env.MOCK_SCENARIO || 'default',
  failureRate: parseFloat(process.env.FAILURE_RATE || 0),
  latencyMs: parseInt(process.env.LATENCY_MS || 0),
  chaosEnabled: process.env.CHAOS_ENABLED === 'true',
  networkId: parseInt(process.env.BLOCKCHAIN_NETWORK_ID || 31337),
  initialBalance: process.env.INITIAL_BALANCE || '1000000000000000000000', // 1000 ETH in wei
  blockTime: parseInt(process.env.BLOCK_TIME || 3000), // 3 seconds
};

// Setup logging
const logger = winston.createLogger({
  level: 'info',
  format: winston.format.combine(
    winston.format.timestamp(),
    winston.format.json()
  ),
  transports: [
    new winston.transports.Console(),
    new winston.transports.File({ filename: '/app/logs/blockchain-mock.log' }),
  ],
});

// Load scenario configurations
let scenarios = {};
try {
  if (fs.existsSync('./scenarios/blockchain-scenarios.json')) {
    scenarios = JSON.parse(
      fs.readFileSync('./scenarios/blockchain-scenarios.json', 'utf8')
    );
  }
} catch (error) {
  logger.warn('Failed to load blockchain scenarios:', error.message);
}

// Blockchain state
let blockchainState = {
  blockNumber: 0,
  blocks: [],
  transactions: {},
  accounts: {},
  contracts: {},
  pendingTxs: [],
  gasPrice: '20000000000', // 20 gwei
  gasLimit: '6721975',
  networkId: config.networkId,
};

// Metrics
const metrics = {
  requests: 0,
  errors: 0,
  responseTime: [],
  methodCounts: {},
  startTime: Date.now(),
};

// Middleware
app.use(cors());
app.use(express.json());

// Request logging middleware
app.use((req, res, next) => {
  logger.info(`RPC Request: ${req.body?.method || 'unknown'}`, {
    ip: req.ip,
    method: req.body?.method,
    id: req.body?.id,
    scenario: config.scenario,
  });
  next();
});

// Failure simulation middleware
app.use((req, res, next) => {
  // Simulate random failures
  if (config.failureRate > 0 && Math.random() < config.failureRate) {
    const errorResponses = [
      { code: -32603, message: 'Internal error' },
      { code: -32000, message: 'Server error' },
      { code: -32601, message: 'Method not found' },
      { code: -32602, message: 'Invalid params' },
    ];
    const error =
      errorResponses[Math.floor(Math.random() * errorResponses.length)];
    logger.warn('Simulating RPC failure', { error, method: req.body?.method });

    return res.json({
      jsonrpc: '2.0',
      error,
      id: req.body?.id || null,
    });
  }

  // Simulate latency
  if (config.latencyMs > 0) {
    setTimeout(next, config.latencyMs);
  } else {
    next();
  }
});

// Metrics middleware
app.use((req, res, next) => {
  const start = Date.now();
  metrics.requests++;

  const method = req.body?.method;
  if (method) {
    metrics.methodCounts[method] = (metrics.methodCounts[method] || 0) + 1;
  }

  res.on('finish', () => {
    const duration = Date.now() - start;
    metrics.responseTime.push(duration);

    // Keep only last 1000 response times
    if (metrics.responseTime.length > 1000) {
      metrics.responseTime = metrics.responseTime.slice(-1000);
    }

    if (res.statusCode >= 400) {
      metrics.errors++;
    }
  });

  next();
});

// Utility functions
const generateHex = length =>
  '0x' + Math.random().toString(16).substr(2, length);
const toHex = num => '0x' + num.toString(16);
const fromHex = hex => parseInt(hex, 16);
const padHex = (hex, length) => '0x' + hex.slice(2).padStart(length, '0');

// Generate realistic Ethereum address
const generateAddress = () => {
  return (
    '0x' +
    Array(40)
      .fill()
      .map(() => Math.floor(Math.random() * 16).toString(16))
      .join('')
  );
};

// Generate transaction hash
const generateTxHash = () => {
  return (
    '0x' +
    Array(64)
      .fill()
      .map(() => Math.floor(Math.random() * 16).toString(16))
      .join('')
  );
};

// Initialize blockchain state
const initializeBlockchain = () => {
  // Genesis block
  const genesisBlock = {
    number: toHex(0),
    hash: '0x' + '0'.repeat(64),
    parentHash: '0x' + '0'.repeat(64),
    timestamp: toHex(Math.floor(Date.now() / 1000)),
    gasLimit: toHex(config.gasLimit || 6721975),
    gasUsed: '0x0',
    transactions: [],
    miner: generateAddress(),
    difficulty: '0x1',
    totalDifficulty: '0x1',
  };

  blockchainState.blocks = [genesisBlock];
  blockchainState.blockNumber = 0;

  // Create initial accounts with balances
  const scenarioConfig = scenarios[config.scenario] || {};
  const accountCount = scenarioConfig.accountCount || 10;

  for (let i = 0; i < accountCount; i++) {
    const address = generateAddress();
    blockchainState.accounts[address] = {
      balance: config.initialBalance,
      nonce: 0,
      code: '0x',
      storage: {},
    };
  }

  logger.info('Blockchain state initialized', {
    scenario: config.scenario,
    blockNumber: blockchainState.blockNumber,
    accounts: Object.keys(blockchainState.accounts).length,
    initialBalance: config.initialBalance,
  });
};

// Mine a new block
const mineBlock = () => {
  const prevBlock = blockchainState.blocks[blockchainState.blocks.length - 1];
  const newBlockNumber = blockchainState.blockNumber + 1;

  // Process pending transactions
  const blockTxs = blockchainState.pendingTxs.splice(0, 100); // Max 100 txs per block

  const newBlock = {
    number: toHex(newBlockNumber),
    hash: generateHex(64),
    parentHash: prevBlock.hash,
    timestamp: toHex(Math.floor(Date.now() / 1000)),
    gasLimit: toHex(config.gasLimit || 6721975),
    gasUsed: toHex(blockTxs.length * 21000), // Simple gas calculation
    transactions: blockTxs.map(tx => tx.hash),
    miner: generateAddress(),
    difficulty: '0x1',
    totalDifficulty: toHex(newBlockNumber),
  };

  blockchainState.blocks.push(newBlock);
  blockchainState.blockNumber = newBlockNumber;

  // Update transaction receipts
  blockTxs.forEach(tx => {
    blockchainState.transactions[tx.hash] = {
      ...tx,
      blockNumber: newBlock.number,
      blockHash: newBlock.hash,
      transactionIndex: '0x0',
      status: '0x1', // Success
      gasUsed: '0x5208', // 21000 gas
    };
  });

  logger.debug('New block mined', {
    blockNumber: newBlockNumber,
    txCount: blockTxs.length,
    hash: newBlock.hash,
  });
};

// JSON-RPC method handlers
const rpcMethods = {
  // Network methods
  net_version: () => config.networkId.toString(),
  net_peerCount: () => toHex(Math.floor(Math.random() * 25) + 5),
  net_listening: () => true,

  // Ethereum methods
  eth_protocolVersion: () => '0x41', // Version 65
  eth_syncing: () => false,
  eth_coinbase: () => generateAddress(),
  eth_mining: () => true,
  eth_hashrate: () => toHex(Math.floor(Math.random() * 1000000)),
  eth_gasPrice: () => blockchainState.gasPrice,
  eth_accounts: () => Object.keys(blockchainState.accounts).slice(0, 10),

  // Block methods
  eth_blockNumber: () => toHex(blockchainState.blockNumber),

  eth_getBlockByNumber: params => {
    const [blockNumber, fullTxs] = params;
    let blockNum;

    if (blockNumber === 'latest') {
      blockNum = blockchainState.blockNumber;
    } else if (blockNumber === 'earliest') {
      blockNum = 0;
    } else if (blockNumber === 'pending') {
      blockNum = blockchainState.blockNumber + 1;
    } else {
      blockNum = fromHex(blockNumber);
    }

    if (blockNum < 0 || blockNum >= blockchainState.blocks.length) {
      return null;
    }

    const block = blockchainState.blocks[blockNum];

    if (fullTxs) {
      // Return full transaction objects
      block.transactions = block.transactions.map(
        txHash =>
          blockchainState.transactions[txHash] || {
            hash: txHash,
            blockNumber: block.number,
            blockHash: block.hash,
            transactionIndex: '0x0',
            from: generateAddress(),
            to: generateAddress(),
            value: '0x0',
            gas: '0x5208',
            gasPrice: blockchainState.gasPrice,
            nonce: '0x0',
            input: '0x',
          }
      );
    }

    return block;
  },

  eth_getBlockByHash: params => {
    const [blockHash, fullTxs] = params;
    const block = blockchainState.blocks.find(b => b.hash === blockHash);

    if (!block) return null;

    if (fullTxs) {
      block.transactions = block.transactions.map(
        txHash =>
          blockchainState.transactions[txHash] || {
            hash: txHash,
            blockNumber: block.number,
            blockHash: block.hash,
            from: generateAddress(),
            to: generateAddress(),
            value: '0x0',
          }
      );
    }

    return block;
  },

  // Account methods
  eth_getBalance: params => {
    const [address, blockNumber] = params;
    const account = blockchainState.accounts[address.toLowerCase()];
    return account ? account.balance : '0x0';
  },

  eth_getTransactionCount: params => {
    const [address, blockNumber] = params;
    const account = blockchainState.accounts[address.toLowerCase()];
    return toHex(account ? account.nonce : 0);
  },

  eth_getCode: params => {
    const [address, blockNumber] = params;
    const account = blockchainState.accounts[address.toLowerCase()];
    return account ? account.code : '0x';
  },

  // Transaction methods
  eth_sendTransaction: params => {
    const [txData] = params;
    const txHash = generateTxHash();

    const transaction = {
      hash: txHash,
      from: txData.from,
      to: txData.to || null,
      value: txData.value || '0x0',
      gas: txData.gas || '0x5208',
      gasPrice: txData.gasPrice || blockchainState.gasPrice,
      nonce: txData.nonce || '0x0',
      input: txData.input || txData.data || '0x',
      blockNumber: null,
      blockHash: null,
      transactionIndex: null,
    };

    blockchainState.pendingTxs.push(transaction);

    logger.info('Transaction submitted', {
      txHash,
      from: txData.from,
      to: txData.to,
    });

    return txHash;
  },

  eth_sendRawTransaction: params => {
    const [signedTxData] = params;
    const txHash = generateTxHash();

    const transaction = {
      hash: txHash,
      from: generateAddress(),
      to: generateAddress(),
      value: '0x0',
      gas: '0x5208',
      gasPrice: blockchainState.gasPrice,
      nonce: '0x0',
      input: '0x',
      blockNumber: null,
      blockHash: null,
      transactionIndex: null,
    };

    blockchainState.pendingTxs.push(transaction);

    return txHash;
  },

  eth_getTransactionByHash: params => {
    const [txHash] = params;
    return blockchainState.transactions[txHash] || null;
  },

  eth_getTransactionReceipt: params => {
    const [txHash] = params;
    const tx = blockchainState.transactions[txHash];

    if (!tx || !tx.blockNumber) return null;

    return {
      transactionHash: txHash,
      transactionIndex: tx.transactionIndex || '0x0',
      blockNumber: tx.blockNumber,
      blockHash: tx.blockHash,
      from: tx.from,
      to: tx.to,
      contractAddress: tx.to ? null : generateAddress(),
      cumulativeGasUsed: tx.gasUsed || '0x5208',
      gasUsed: tx.gasUsed || '0x5208',
      status: tx.status || '0x1',
      logs: [],
      logsBloom: '0x' + '0'.repeat(512),
    };
  },

  // Contract methods
  eth_call: params => {
    const [txData, blockNumber] = params;

    // Simulate contract call results based on method signature
    const input = txData.input || txData.data || '0x';

    // Common method signatures
    const methodSigs = {
      '0x70a08231':
        '0x' +
        Math.floor(Math.random() * 1000000)
          .toString(16)
          .padStart(64, '0'), // balanceOf
      '0x18160ddd': '0x' + (1000000).toString(16).padStart(64, '0'), // totalSupply
      '0x95d89b41': '0x' + Buffer.from('WRF').toString('hex').padEnd(64, '0'), // symbol
      '0x06fdde03':
        '0x' + Buffer.from('WRFCoin').toString('hex').padEnd(64, '0'), // name
      '0x313ce567': '0x' + (18).toString(16).padStart(64, '0'), // decimals
    };

    const methodSig = input.slice(0, 10);
    return methodSigs[methodSig] || '0x';
  },

  eth_estimateGas: params => {
    const [txData] = params;
    // Simple gas estimation
    const baseGas = 21000;
    const dataGas = txData.input ? ((txData.input.length - 2) / 2) * 16 : 0;
    return toHex(baseGas + dataGas);
  },

  // Filter and log methods
  eth_newFilter: () => toHex(Math.floor(Math.random() * 1000000)),
  eth_newBlockFilter: () => toHex(Math.floor(Math.random() * 1000000)),
  eth_newPendingTransactionFilter: () =>
    toHex(Math.floor(Math.random() * 1000000)),
  eth_uninstallFilter: () => true,
  eth_getFilterChanges: () => [],
  eth_getFilterLogs: () => [],
  eth_getLogs: () => [],

  // Debug methods (for testing)
  debug_getBlockchainState: () => ({
    blockNumber: blockchainState.blockNumber,
    totalBlocks: blockchainState.blocks.length,
    totalAccounts: Object.keys(blockchainState.accounts).length,
    pendingTxs: blockchainState.pendingTxs.length,
    totalTxs: Object.keys(blockchainState.transactions).length,
  }),

  debug_resetBlockchain: () => {
    initializeBlockchain();
    return {
      status: 'reset_complete',
      blockNumber: blockchainState.blockNumber,
    };
  },
};

// Health check endpoint
app.get('/health', (req, res) => {
  const uptime = Date.now() - metrics.startTime;
  const avgResponseTime =
    metrics.responseTime.length > 0
      ? metrics.responseTime.reduce((a, b) => a + b, 0) /
        metrics.responseTime.length
      : 0;

  const health = {
    status: 'healthy',
    service: 'blockchain-rpc-mock',
    version: '2.0.0',
    scenario: config.scenario,
    uptime: Math.floor(uptime / 1000),
    blockchain: {
      networkId: config.networkId,
      blockNumber: blockchainState.blockNumber,
      totalBlocks: blockchainState.blocks.length,
      pendingTxs: blockchainState.pendingTxs.length,
      accounts: Object.keys(blockchainState.accounts).length,
    },
    metrics: {
      totalRequests: metrics.requests,
      totalErrors: metrics.errors,
      errorRate:
        metrics.requests > 0
          ? ((metrics.errors / metrics.requests) * 100).toFixed(2) + '%'
          : '0%',
      averageResponseTime: Math.round(avgResponseTime) + 'ms',
      popularMethods: Object.entries(metrics.methodCounts)
        .sort(([, a], [, b]) => b - a)
        .slice(0, 5)
        .map(([method, count]) => ({ method, count })),
    },
    configuration: {
      failureRate: config.failureRate,
      latencyMs: config.latencyMs,
      chaosEnabled: config.chaosEnabled,
      blockTime: config.blockTime,
    },
    timestamp: new Date().toISOString(),
  };

  res.json(health);
});

// Metrics endpoint
app.get('/metrics', (req, res) => {
  const uptime = Date.now() - metrics.startTime;
  const avgResponseTime =
    metrics.responseTime.length > 0
      ? metrics.responseTime.reduce((a, b) => a + b, 0) /
        metrics.responseTime.length
      : 0;

  const prometheusMetrics = `
# HELP blockchain_mock_requests_total Total number of RPC requests
# TYPE blockchain_mock_requests_total counter
blockchain_mock_requests_total ${metrics.requests}

# HELP blockchain_mock_errors_total Total number of errors
# TYPE blockchain_mock_errors_total counter
blockchain_mock_errors_total ${metrics.errors}

# HELP blockchain_mock_response_time_ms Average response time in milliseconds
# TYPE blockchain_mock_response_time_ms gauge
blockchain_mock_response_time_ms ${Math.round(avgResponseTime)}

# HELP blockchain_mock_uptime_seconds Service uptime in seconds
# TYPE blockchain_mock_uptime_seconds counter
blockchain_mock_uptime_seconds ${Math.floor(uptime / 1000)}

# HELP blockchain_mock_block_number Current block number
# TYPE blockchain_mock_block_number gauge
blockchain_mock_block_number ${blockchainState.blockNumber}

# HELP blockchain_mock_pending_transactions Pending transactions count
# TYPE blockchain_mock_pending_transactions gauge
blockchain_mock_pending_transactions ${blockchainState.pendingTxs.length}
`;

  res.set('Content-Type', 'text/plain');
  res.send(prometheusMetrics.trim());
});

// Main JSON-RPC endpoint
app.post('/', (req, res) => {
  const { jsonrpc, method, params, id } = req.body;

  // Validate JSON-RPC format
  if (jsonrpc !== '2.0') {
    return res.json({
      jsonrpc: '2.0',
      error: { code: -32600, message: 'Invalid Request' },
      id,
    });
  }

  if (!method) {
    return res.json({
      jsonrpc: '2.0',
      error: { code: -32602, message: 'Invalid params' },
      id,
    });
  }

  // Execute RPC method
  const handler = rpcMethods[method];
  if (!handler) {
    logger.warn('Unknown RPC method', { method });
    return res.json({
      jsonrpc: '2.0',
      error: { code: -32601, message: 'Method not found' },
      id,
    });
  }

  try {
    const result = handler(params || []);

    res.json({
      jsonrpc: '2.0',
      result,
      id,
    });
  } catch (error) {
    logger.error('RPC method error', { method, error: error.message });
    res.json({
      jsonrpc: '2.0',
      error: { code: -32603, message: 'Internal error' },
      id,
    });
  }
});

// Scenario management
app.get('/api/mock/scenario', (req, res) => {
  res.json({
    success: true,
    data: {
      currentScenario: config.scenario,
      availableScenarios: Object.keys(scenarios),
      blockchainState: {
        blockNumber: blockchainState.blockNumber,
        accounts: Object.keys(blockchainState.accounts).length,
        pendingTxs: blockchainState.pendingTxs.length,
      },
      configuration: config,
    },
    timestamp: new Date().toISOString(),
  });
});

// Chaos engineering endpoint
app.post('/api/mock/chaos', (req, res) => {
  const { action, duration = 30 } = req.body;

  switch (action) {
    case 'network_partition':
      config.failureRate = 0.8;
      setTimeout(() => {
        config.failureRate = 0;
      }, duration * 1000);
      break;

    case 'slow_blocks':
      config.blockTime = config.blockTime * 10;
      setTimeout(() => {
        config.blockTime = 3000;
      }, duration * 1000);
      break;

    case 'tx_pool_overflow':
      // Add many pending transactions
      for (let i = 0; i < 1000; i++) {
        blockchainState.pendingTxs.push({
          hash: generateTxHash(),
          from: generateAddress(),
          to: generateAddress(),
          value: '0x0',
        });
      }
      break;

    default:
      return res.status(400).json({
        success: false,
        error: 'Invalid chaos action',
        supportedActions: [
          'network_partition',
          'slow_blocks',
          'tx_pool_overflow',
        ],
        timestamp: new Date().toISOString(),
      });
  }

  logger.warn('Chaos engineering activated', { action, duration });

  res.json({
    success: true,
    message: `Chaos action '${action}' activated for ${duration} seconds`,
    timestamp: new Date().toISOString(),
  });
});

// Error handling
app.use((req, res) => {
  res.status(404).json({
    success: false,
    error: `Endpoint ${req.method} ${req.path} not found`,
    timestamp: new Date().toISOString(),
  });
});

app.use((error, req, res, next) => {
  logger.error('Blockchain RPC Mock Error:', error);
  res.status(500).json({
    success: false,
    error: 'Internal server error',
    timestamp: new Date().toISOString(),
  });
});

// Block mining simulation
setInterval(() => {
  if (blockchainState.pendingTxs.length > 0) {
    mineBlock();
  }
}, config.blockTime);

// Initialize and start server
initializeBlockchain();

app.listen(PORT, '0.0.0.0', () => {
  logger.info('Enhanced Blockchain RPC Mock started', {
    port: PORT,
    scenario: config.scenario,
    networkId: config.networkId,
    blockNumber: blockchainState.blockNumber,
    accounts: Object.keys(blockchainState.accounts).length,
    config,
  });
});

module.exports = app;
