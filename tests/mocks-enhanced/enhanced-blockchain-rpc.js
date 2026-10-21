#!/usr/bin/env node

/**
 * Enhanced Blockchain RPC Mock Server
 * Provides comprehensive Ethereum JSON-RPC interface with advanced features
 * Supports WebSocket connections, failure simulation, and edge case testing
 */

const express = require('express');
const cors = require('cors');
const WebSocket = require('ws');
const { v4: uuidv4 } = require('uuid');

const app = express();
const PORT = process.env.PORT || 8545;
const WS_PORT = process.env.WS_PORT || 8546;
const FAILOVER_PORT = process.env.FAILOVER_PORT || 8547;

// Configuration
const config = {
  chainId: process.env.CHAIN_ID || '0x1337',
  blockTimeMs: parseInt(process.env.BLOCK_TIME_MS) || 1000,
  gasPriceGwei: parseInt(process.env.GAS_PRICE_GWEI) || 20,
  maxGasLimit: parseInt(process.env.MAX_GAS_LIMIT) || 8000000,
  enableMining: process.env.ENABLE_MINING === 'true',
  simulateNetworkDelays: process.env.SIMULATE_NETWORK_DELAYS === 'true',
  failureRatePercent: parseInt(process.env.FAILURE_RATE_PERCENT) || 0,
  randomErrors: process.env.RANDOM_ERRORS === 'true',
  timeoutScenarios: process.env.TIMEOUT_SCENARIOS === 'true',
};

// State management
let blockNumber = 1000000;
let transactionNonce = 0;
let gasPrice = BigInt(config.gasPriceGwei) * BigInt(1000000000); // Convert to wei

// Mock accounts with different balance scenarios
const mockAccounts = [
  {
    address: '0x742d35Cc6634C0532925a3b8D8aE7b8e4B9f1234',
    balance: '1000000000000000000000',
    name: 'whale',
  },
  {
    address: '0x856f48C9247d7b8e3D4B8F5e2C9A7b8E4C5F1234',
    balance: '500000000000000000000',
    name: 'normal',
  },
  {
    address: '0x123f48C9247d7b8e3D4B8F5e2C9A7b8E4C5F5678',
    balance: '250000000000000000000',
    name: 'small',
  },
  {
    address: '0x000000000000000000000000000000000000dead',
    balance: '0',
    name: 'empty',
  },
  {
    address: '0x456f48C9247d7b8e3D4B8F5e2C9A7b8E4C5F9999',
    balance: '1',
    name: 'dust',
  },
];

// Transaction and block storage
const transactions = new Map();
const blocks = new Map();
const receipts = new Map();
const pendingTransactions = new Map();

// WebSocket connections
const wsConnections = new Set();

// Middleware
app.use(
  cors({
    origin: '*',
    methods: ['GET', 'POST', 'OPTIONS'],
    allowedHeaders: ['Content-Type', 'Authorization'],
  })
);
app.use(express.json());

// Utility functions
function generateHash() {
  return (
    '0x' +
    Array.from({ length: 64 }, () =>
      Math.floor(Math.random() * 16).toString(16)
    ).join('')
  );
}

function toHex(num) {
  return '0x' + num.toString(16);
}

function fromHex(hex) {
  return parseInt(hex, 16);
}

function simulateNetworkDelay() {
  if (config.simulateNetworkDelays) {
    const delay = Math.random() * 100; // 0-100ms
    return new Promise(resolve => setTimeout(resolve, delay));
  }
  return Promise.resolve();
}

function shouldSimulateFailure() {
  return Math.random() * 100 < config.failureRatePercent;
}

function getRandomError() {
  const errors = [
    { code: -32000, message: 'Server error: transaction pool full' },
    { code: -32001, message: 'Server error: network congestion' },
    { code: -32002, message: 'Server error: insufficient funds for gas' },
    { code: -32003, message: 'Server error: transaction underpriced' },
    { code: -32004, message: 'Server error: nonce too low' },
    {
      code: -32005,
      message: 'Server error: replacement transaction underpriced',
    },
    { code: -32006, message: 'Server error: known transaction' },
    { code: -32007, message: 'Server error: execution reverted' },
  ];
  return errors[Math.floor(Math.random() * errors.length)];
}

// Mock block generation
function generateBlock(includeTransactions = false) {
  const block = {
    number: toHex(blockNumber),
    hash: generateHash(),
    parentHash: blocks.get(blockNumber - 1)?.hash || generateHash(),
    nonce: toHex(Math.floor(Math.random() * 1000000)),
    sha3Uncles:
      '0x1dcc4de8dec75d7aab85b567b6ccd41ad312451b948a7413f0a142fd40d49347',
    logsBloom: '0x' + '0'.repeat(512),
    transactionsRoot: generateHash(),
    stateRoot: generateHash(),
    receiptsRoot: generateHash(),
    miner: mockAccounts[0].address,
    difficulty: toHex(1),
    totalDifficulty: toHex(blockNumber),
    extraData: '0x',
    size: toHex(Math.floor(Math.random() * 10000) + 1000),
    gasLimit: toHex(config.maxGasLimit),
    gasUsed: toHex(Math.floor(Math.random() * config.maxGasLimit * 0.8)),
    timestamp: toHex(Math.floor(Date.now() / 1000)),
    transactions: includeTransactions ? [] : [],
    uncles: [],
  };

  blocks.set(blockNumber, block);
  return block;
}

// JSON-RPC method handlers
const rpcMethods = {
  // Network information
  net_version: () => fromHex(config.chainId).toString(),
  eth_chainId: () => config.chainId,
  eth_protocolVersion: () => '0x41',

  // Block information
  eth_blockNumber: () => toHex(blockNumber),

  eth_getBlockByNumber: (blockNum, includeTransactions) => {
    const num = blockNum === 'latest' ? blockNumber : fromHex(blockNum);
    return blocks.get(num) || generateBlock(includeTransactions);
  },

  eth_getBlockByHash: (blockHash, includeTransactions) => {
    for (let block of blocks.values()) {
      if (block.hash === blockHash) {
        return block;
      }
    }
    return generateBlock(includeTransactions);
  },

  // Account information
  eth_accounts: () => mockAccounts.map(acc => acc.address),

  eth_getBalance: (address, blockNum) => {
    const account = mockAccounts.find(
      acc => acc.address.toLowerCase() === address.toLowerCase()
    );
    if (account) return account.balance;

    // Generate random balance for unknown accounts
    const randomBalance = Math.floor(Math.random() * 1000000000000000000000);
    return randomBalance.toString();
  },

  eth_getTransactionCount: (address, blockNum) => {
    // Simple nonce simulation
    return toHex(Math.floor(Math.random() * 100));
  },

  // Transaction operations
  eth_sendTransaction: tx => {
    if (shouldSimulateFailure()) {
      throw getRandomError();
    }

    const txHash = generateHash();
    const transaction = {
      hash: txHash,
      nonce: toHex(transactionNonce++),
      blockHash: null, // Pending
      blockNumber: null,
      transactionIndex: null,
      from: tx.from,
      to: tx.to || null,
      value: tx.value || '0x0',
      gas: tx.gas || toHex(21000),
      gasPrice: tx.gasPrice || toHex(gasPrice),
      input: tx.data || '0x',
      creates: tx.to ? null : generateHash().slice(0, 42),
      raw: '0x' + Math.random().toString(16).slice(2),
      publicKey: generateHash(),
      r: generateHash(),
      s: generateHash(),
      v: toHex(27 + Math.floor(Math.random() * 2)),
    };

    pendingTransactions.set(txHash, transaction);

    // Broadcast to WebSocket connections
    wsConnections.forEach(ws => {
      if (ws.readyState === WebSocket.OPEN) {
        ws.send(
          JSON.stringify({
            jsonrpc: '2.0',
            method: 'eth_subscription',
            params: {
              subscription: 'pending_transactions',
              result: txHash,
            },
          })
        );
      }
    });

    return txHash;
  },

  eth_sendRawTransaction: rawTx => {
    return rpcMethods.eth_sendTransaction({});
  },

  eth_getTransactionByHash: txHash => {
    return transactions.get(txHash) || pendingTransactions.get(txHash) || null;
  },

  eth_getTransactionReceipt: txHash => {
    return receipts.get(txHash) || null;
  },

  // Gas estimation
  eth_gasPrice: () => toHex(gasPrice),
  eth_maxPriorityFeePerGas: () => toHex(1000000000), // 1 gwei
  eth_feeHistory: () => ({
    baseFeePerGas: [toHex(gasPrice)],
    gasUsedRatio: [0.5],
    reward: [[toHex(1000000000)]],
  }),

  eth_estimateGas: tx => {
    // Simulate different gas estimates based on transaction type
    if (tx.to === null) return toHex(200000); // Contract creation
    if (tx.data && tx.data !== '0x') return toHex(50000); // Contract call
    return toHex(21000); // Simple transfer
  },

  // Smart contract interaction
  eth_call: (tx, blockNum) => {
    // Simulate various contract call responses
    if (tx.data && tx.data.startsWith('0x70a08231')) {
      // balanceOf(address)
      return '0x' + Math.floor(Math.random() * 1000000).toString(16);
    }
    if (tx.data && tx.data.startsWith('0x18160ddd')) {
      // totalSupply()
      return '0x' + (1000000000 * Math.random()).toString(16);
    }
    return '0x' + Math.random().toString(16).slice(2, 66); // Random 32-byte response
  },

  // Subscription methods (for WebSocket)
  eth_subscribe: (subscriptionType, params) => {
    const subscriptionId = uuidv4();
    return subscriptionId;
  },

  eth_unsubscribe: subscriptionId => {
    return true;
  },

  // Mining simulation
  eth_mining: () => config.enableMining,
  eth_hashrate: () =>
    config.enableMining ? toHex(Math.floor(Math.random() * 1000000)) : '0x0',

  // Logs and events
  eth_getLogs: filter => {
    return []; // Return empty array for now
  },

  // Debug methods
  debug_traceTransaction: (txHash, options) => {
    return {
      gas: Math.floor(Math.random() * 100000),
      failed: false,
      returnValue: '',
      structLogs: [],
    };
  },

  // Test-specific methods
  test_setTime: timestamp => {
    // Allow time manipulation for testing
    return true;
  },

  test_mine: numBlocks => {
    // Mine blocks on demand
    for (let i = 0; i < (numBlocks || 1); i++) {
      blockNumber++;
      generateBlock();
    }
    return blockNumber;
  },

  test_reset: () => {
    // Reset blockchain state
    blockNumber = 1000000;
    transactionNonce = 0;
    transactions.clear();
    blocks.clear();
    receipts.clear();
    pendingTransactions.clear();
    return true;
  },
};

// Health check endpoint
app.get('/health', async (req, res) => {
  await simulateNetworkDelay();

  res.json({
    status: 'healthy',
    service: process.env.SERVICE_NAME || 'blockchain-rpc-enhanced',
    timestamp: new Date().toISOString(),
    config: {
      chainId: config.chainId,
      blockNumber,
      failureRate: config.failureRatePercent + '%',
    },
    endpoints: {
      jsonrpc: `http://localhost:${PORT}`,
      websocket: `ws://localhost:${WS_PORT}`,
      failover: `http://localhost:${FAILOVER_PORT}`,
    },
  });
});

// WebSocket endpoint info
app.get('/ws', (req, res) => {
  res.json({
    websocket_url: `ws://localhost:${WS_PORT}`,
    supported_subscriptions: [
      'newHeads',
      'pendingTransactions',
      'logs',
      'syncing',
    ],
  });
});

// Stats endpoint
app.get('/api/stats', (req, res) => {
  res.json({
    network: 'WRFCoin Test Network',
    chainId: config.chainId,
    blockNumber,
    pendingTransactions: pendingTransactions.size,
    totalTransactions: transactions.size,
    accounts: mockAccounts.length,
    gasPrice: gasPrice.toString(),
    isHealthy: true,
    uptime: process.uptime(),
  });
});

// Main JSON-RPC endpoint
app.post('/', async (req, res) => {
  await simulateNetworkDelay();

  // Simulate timeout scenarios
  if (config.timeoutScenarios && Math.random() < 0.05) {
    // 5% chance of timeout
    await new Promise(resolve => setTimeout(resolve, 30000));
  }

  // Handle batch requests
  const requests = Array.isArray(req.body) ? req.body : [req.body];
  const responses = [];

  for (const request of requests) {
    try {
      // Simulate random failures
      if (config.randomErrors && shouldSimulateFailure()) {
        const error = getRandomError();
        responses.push({
          jsonrpc: '2.0',
          id: request.id,
          error,
        });
        continue;
      }

      const { method, params = [], id } = request;

      if (!rpcMethods[method]) {
        responses.push({
          jsonrpc: '2.0',
          id,
          error: { code: -32601, message: 'Method not found' },
        });
        continue;
      }

      const result = rpcMethods[method](...params);
      responses.push({
        jsonrpc: '2.0',
        id,
        result,
      });
    } catch (error) {
      responses.push({
        jsonrpc: '2.0',
        id: request.id,
        error:
          typeof error === 'object' && error.code
            ? error
            : { code: -32603, message: error.message || 'Internal error' },
      });
    }
  }

  res.json(Array.isArray(req.body) ? responses : responses[0]);
});

// WebSocket server
const wss = new WebSocket.Server({ port: WS_PORT });

wss.on('connection', ws => {
  console.log('WebSocket connection established');
  wsConnections.add(ws);

  ws.on('message', async data => {
    try {
      const request = JSON.parse(data);
      const { method, params = [], id } = request;

      if (method === 'eth_subscribe') {
        const subscriptionId = uuidv4();
        ws.send(
          JSON.stringify({
            jsonrpc: '2.0',
            id,
            result: subscriptionId,
          })
        );
      } else if (rpcMethods[method]) {
        const result = rpcMethods[method](...params);
        ws.send(
          JSON.stringify({
            jsonrpc: '2.0',
            id,
            result,
          })
        );
      }
    } catch (error) {
      ws.send(
        JSON.stringify({
          jsonrpc: '2.0',
          id: null,
          error: { code: -32700, message: 'Parse error' },
        })
      );
    }
  });

  ws.on('close', () => {
    console.log('WebSocket connection closed');
    wsConnections.delete(ws);
  });
});

// Block mining simulation
if (config.enableMining) {
  setInterval(() => {
    blockNumber++;
    const block = generateBlock();

    // Move pending transactions to mined
    const pendingTxs = Array.from(pendingTransactions.entries()).slice(0, 5);
    pendingTxs.forEach(([hash, tx]) => {
      tx.blockNumber = toHex(blockNumber);
      tx.blockHash = block.hash;
      tx.transactionIndex = toHex(transactions.size);

      transactions.set(hash, tx);
      pendingTransactions.delete(hash);

      // Create receipt
      receipts.set(hash, {
        transactionHash: hash,
        transactionIndex: tx.transactionIndex,
        blockNumber: tx.blockNumber,
        blockHash: tx.blockHash,
        cumulativeGasUsed: toHex(21000),
        gasUsed: toHex(21000),
        contractAddress: tx.creates,
        logs: [],
        logsBloom: '0x' + '0'.repeat(512),
        status: '0x1',
      });
    });

    // Broadcast new block to WebSocket subscribers
    wsConnections.forEach(ws => {
      if (ws.readyState === WebSocket.OPEN) {
        ws.send(
          JSON.stringify({
            jsonrpc: '2.0',
            method: 'eth_subscription',
            params: {
              subscription: 'newHeads',
              result: block,
            },
          })
        );
      }
    });
  }, config.blockTimeMs);
}

// Start servers
app.listen(PORT, '0.0.0.0', () => {
  console.log(`Enhanced Blockchain RPC Mock Server running on port ${PORT}`);
  console.log(`WebSocket server running on port ${WS_PORT}`);
  console.log(`Chain ID: ${config.chainId}`);
  console.log(`Failure rate: ${config.failureRatePercent}%`);
  console.log(`Mining enabled: ${config.enableMining}`);
});

// Failover server (if enabled)
if (FAILOVER_PORT !== PORT) {
  const failoverApp = express();
  failoverApp.use(cors());
  failoverApp.use(express.json());

  failoverApp.get('/health', (req, res) => {
    res.json({
      status: 'healthy',
      service: 'blockchain-rpc-failover',
      timestamp: new Date().toISOString(),
    });
  });

  failoverApp.post('/', (req, res) => {
    // Simple proxy to main server
    res.json({
      jsonrpc: '2.0',
      id: req.body.id,
      result: '0x0',
    });
  });

  failoverApp.listen(FAILOVER_PORT, '0.0.0.0', () => {
    console.log(`Failover server running on port ${FAILOVER_PORT}`);
  });
}

process.on('SIGTERM', () => {
  console.log('Shutting down servers...');
  wss.close();
  process.exit(0);
});
