const express = require('express');
const cors = require('cors');

const app = express();
const PORT = process.env.PORT || 8545;

app.use(cors());
app.use(express.json());

// Mock blockchain state
let blockNumber = 1000000;
let transactionCount = 0;
const mockAccounts = [
  '0x742d35Cc6634C0532925a3b8D8aE7b8e4B9f1234',
  '0x856f48C9247d7b8e3D4B8F5e2C9A7b8E4C5F1234',
  '0x123f48C9247d7b8e3D4B8F5e2C9A7b8E4C5F5678',
];

const mockBalances = {
  '0x742d35Cc6634C0532925a3b8D8aE7b8e4B9f1234': '1000000000000000000000',
  '0x856f48C9247d7b8e3D4B8F5e2C9A7b8E4C5F1234': '500000000000000000000',
  '0x123f48C9247d7b8e3D4B8F5e2C9A7b8E4C5F5678': '250000000000000000000',
};

const mockTransactions = new Map();
const mockBlocks = new Map();

// Generate mock transaction hash
function generateTxHash() {
  return (
    '0x' +
    Array.from({ length: 64 }, () =>
      Math.floor(Math.random() * 16).toString(16)
    ).join('')
  );
}

// Generate mock block hash
function generateBlockHash() {
  return (
    '0x' +
    Array.from({ length: 64 }, () =>
      Math.floor(Math.random() * 16).toString(16)
    ).join('')
  );
}

// Health check endpoint
app.get('/health', (req, res) => {
  res.json({
    status: 'healthy',
    service: 'blockchain-rpc-mock',
    timestamp: new Date().toISOString(),
    blockNumber,
    chainId: '0x1337', // Local test chain
  });
});

// Handle JSON-RPC requests
app.post('/', (req, res) => {
  const { method, params = [], id = 1, jsonrpc = '2.0' } = req.body;

  let result;

  try {
    switch (method) {
      case 'eth_chainId':
        result = '0x1337'; // 4919 in decimal (test chain)
        break;

      case 'eth_blockNumber':
        result = `0x${blockNumber.toString(16)}`;
        break;

      case 'eth_accounts':
        result = mockAccounts;
        break;

      case 'eth_getBalance':
        const [address, block] = params;
        result = mockBalances[address] || '0x0';
        break;

      case 'eth_getTransactionCount':
        const [addr, blockTag] = params;
        result = `0x${Math.floor(Math.random() * 100).toString(16)}`;
        break;

      case 'eth_gasPrice':
        result = '0x3b9aca00'; // 1 gwei
        break;

      case 'eth_estimateGas':
        const [txObject] = params;
        result = '0x5208'; // 21000 gas
        break;

      case 'eth_sendTransaction':
        const [transaction] = params;
        const txHash = generateTxHash();
        transactionCount++;

        // Store mock transaction
        mockTransactions.set(txHash, {
          hash: txHash,
          nonce: `0x${transactionCount.toString(16)}`,
          from: transaction.from,
          to: transaction.to,
          value: transaction.value || '0x0',
          gas: transaction.gas || '0x5208',
          gasPrice: transaction.gasPrice || '0x3b9aca00',
          input: transaction.data || '0x',
          blockNumber: `0x${blockNumber.toString(16)}`,
          blockHash: generateBlockHash(),
          transactionIndex: '0x0',
          status: '0x1', // Success
        });

        result = txHash;
        break;

      case 'eth_getTransactionByHash':
        const [txHash2] = params;
        result = mockTransactions.get(txHash2) || null;
        break;

      case 'eth_getTransactionReceipt':
        const [txHash3] = params;
        const tx = mockTransactions.get(txHash3);
        result = tx
          ? {
              ...tx,
              cumulativeGasUsed: '0x5208',
              gasUsed: '0x5208',
              logs: [],
              logsBloom: '0x' + '0'.repeat(512),
              effectiveGasPrice: tx.gasPrice,
            }
          : null;
        break;

      case 'eth_getBlockByNumber':
        const [blockNum, fullTx] = params;
        const blockNumInt = parseInt(blockNum, 16);
        result = {
          number: blockNum,
          hash: generateBlockHash(),
          parentHash: generateBlockHash(),
          timestamp: `0x${Math.floor(Date.now() / 1000).toString(16)}`,
          gasLimit: '0x1c9c380',
          gasUsed: '0x5208',
          miner: mockAccounts[0],
          difficulty: '0x1',
          totalDifficulty: '0x1000',
          size: '0x200',
          transactions: fullTx ? [] : [],
          transactionsRoot: generateBlockHash(),
          stateRoot: generateBlockHash(),
          receiptsRoot: generateBlockHash(),
        };
        break;

      case 'eth_call':
        const [callObject, blockRef] = params;
        // Mock contract call responses based on method signatures
        const methodSig = callObject.data ? callObject.data.slice(0, 10) : '';

        switch (methodSig) {
          case '0x70a08231': // balanceOf(address)
            result =
              '0x' +
              Math.floor(Math.random() * 1000000)
                .toString(16)
                .padStart(64, '0');
            break;
          case '0x18160ddd': // totalSupply()
            result = '0x' + (1000000000).toString(16).padStart(64, '0');
            break;
          case '0x06fdde03': // name()
            result =
              '0x' +
              Buffer.from('WRFCoin Test Token').toString('hex').padEnd(64, '0');
            break;
          default:
            result = '0x' + '0'.repeat(64); // Default empty response
        }
        break;

      case 'net_version':
        result = '4919'; // Test network ID
        break;

      case 'eth_syncing':
        result = false; // Not syncing
        break;

      case 'web3_clientVersion':
        result = 'WRFCoin-TestRPC/v1.0.0';
        break;

      default:
        throw new Error(`Method ${method} not supported`);
    }

    // Increment block number occasionally
    if (Math.random() < 0.1) {
      blockNumber++;
    }

    res.json({
      jsonrpc,
      id,
      result,
    });
  } catch (error) {
    console.error('RPC Error:', error.message);
    res.json({
      jsonrpc,
      id,
      error: {
        code: -32601,
        message: error.message,
      },
    });
  }
});

// WebSocket endpoint for subscriptions (mock)
app.get('/ws', (req, res) => {
  res.status(501).json({
    error: 'WebSocket subscriptions not implemented in mock',
    suggestion: 'Use HTTP polling for testing',
  });
});

// Get mock network stats
app.get('/api/stats', (req, res) => {
  res.json({
    success: true,
    data: {
      chainId: '0x1337',
      blockNumber,
      transactionCount,
      accountCount: mockAccounts.length,
      gasPrice: '0x3b9aca00',
      timestamp: new Date().toISOString(),
    },
  });
});

// Error handling
app.use((req, res) => {
  res.status(404).json({
    jsonrpc: '2.0',
    id: 1,
    error: {
      code: -32601,
      message: `Method not found: ${req.path}`,
    },
  });
});

app.use((error, req, res, next) => {
  console.error('Blockchain RPC Mock Error:', error);
  res.status(500).json({
    jsonrpc: '2.0',
    id: 1,
    error: {
      code: -32603,
      message: 'Internal error',
    },
  });
});

app.listen(PORT, '0.0.0.0', () => {
  console.log(`Blockchain RPC Mock running on port ${PORT}`);
  console.log(`Health check: http://localhost:${PORT}/health`);
  console.log(`Stats endpoint: http://localhost:${PORT}/api/stats`);
});
