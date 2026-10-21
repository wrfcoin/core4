# Transaction Simulation Testing Guide

This guide demonstrates how to use the Wrfcoin Transaction Simulation utilities to test transaction
behavior, measure propagation times, and simulate realistic network conditions.

## Table of Contents

1. [Basic Transaction Simulation](#basic-transaction-simulation)
2. [Transaction Pool Testing](#transaction-pool-testing)
3. [Network Propagation Testing](#network-propagation-testing)
4. [Realistic Transaction Generation](#realistic-transaction-generation)
5. [Advanced Transaction Testing](#advanced-transaction-testing)

## Basic Transaction Simulation

Start by creating and testing basic transactions:

```javascript
const { SimulatedTransaction } = require('../../utils/testing');

// Create a regular transaction
const transaction = new SimulatedTransaction({
  sender: 'WRF1Sender123456789',
  recipient: 'WRF1Recipient123456789',
  amount: 10.5,
  fee: 0.01,
  type: 'Regular',
});

// Validate the transaction
const validationResult = transaction.validate();
console.log('Transaction valid:', validationResult.valid);
if (!validationResult.valid) {
  console.log('Validation errors:', validationResult.errors);
}

// Sign the transaction (simulated)
transaction.sign('privateKeySimulation');
console.log('Transaction signed:', transaction.signature !== null);

// Verify the signature (simulated)
const signatureValid = transaction.verifySignature();
console.log('Signature valid:', signatureValid);

// Serialize for network transmission
const serialized = transaction.serialize();
console.log('Serialized transaction:', serialized);

// Deserialize from network transmission
const deserialized = SimulatedTransaction.deserialize(serialized);
console.log('Deserialized transaction:', deserialized);
```

## Transaction Pool Testing

Simulate transaction pool behavior, including fee prioritization and mempool management:

```javascript
const { SimulatedTransaction, TransactionPoolSimulator } = require('../../utils/testing');

// Create a transaction pool with configuration
const txPool = new TransactionPoolSimulator({
  maxPoolSize: 1000,
  maxTransactionsPerSender: 10,
  minFeePerByte: 0.0001,
  expirationTime: 3600000, // 1 hour in ms
  blockSize: 100, // transactions per block
});

// Create a set of test transactions
const transactions = [
  new SimulatedTransaction({
    sender: 'WRF1Sender123456789',
    recipient: 'WRF1Recipient123456789',
    amount: 5.0,
    fee: 0.05, // High fee
    type: 'Regular',
  }),
  new SimulatedTransaction({
    sender: 'WRF1Sender123456789',
    recipient: 'WRF1Recipient987654321',
    amount: 10.0,
    fee: 0.01, // Medium fee
    type: 'Regular',
  }),
  new SimulatedTransaction({
    sender: 'WRF1Sender987654321',
    recipient: 'WRF1Recipient123456789',
    amount: 15.0,
    fee: 0.002, // Low fee
    type: 'Regular',
  }),
];

// Sign all transactions (simulated)
transactions.forEach(tx => tx.sign('privateKeySimulation'));

// Add transactions to the pool
transactions.forEach(tx => {
  const added = txPool.addTransaction(tx);
  console.log(`Transaction added to pool: ${added}`);
});

// Get pool statistics
const stats = txPool.getPoolStats();
console.log('Transaction pool stats:', stats);

// Get transactions prioritized by fee
const prioritizedTxs = txPool.getPrioritizedTransactions();
console.log('Transactions in fee priority order:');
prioritizedTxs.forEach((tx, index) => {
  console.log(`${index + 1}. ID: ${tx.id}, Fee: ${tx.fee}`);
});

// Simulate creating a block
const blockTransactions = txPool.getTransactionsForBlock();
console.log(`Selected ${blockTransactions.length} transactions for the next block`);

// Check which transactions remain in the pool
console.log('Remaining transactions in pool:', txPool.getPoolSize());

// Check if a specific transaction is in the pool
const isInPool = txPool.hasTransaction(transactions[0].id);
console.log(`Transaction ${transactions[0].id} is in pool: ${isInPool}`);
```

## Network Propagation Testing

Test how transactions propagate through the network:

```javascript
const { SimulatedTransaction, NetworkPropagationSimulator } = require('../../utils/testing');

// Create a network propagation simulator
const networkSim = new NetworkPropagationSimulator({
  nodeCount: 100,
  averageConnections: 8,
  averageLatency: 150, // ms
  bandwidthLimits: {
    min: 1, // Mbps
    max: 10, // Mbps
  },
  geographicDistribution: true,
  packetLossRate: 0.01, // 1% packet loss
});

// Create a test transaction
const transaction = new SimulatedTransaction({
  sender: 'WRF1Sender123456789',
  recipient: 'WRF1Recipient123456789',
  amount: 5.0,
  fee: 0.01,
  type: 'Regular',
  data: {
    weatherData: {
      location: 'Berlin',
      temperature: 22.5,
      timestamp: Date.now(),
    },
  },
});

// Sign the transaction
transaction.sign('privateKeySimulation');

// Broadcast the transaction and track propagation
const sourcePeerId = 'node-1';
const propagationResults = await networkSim.broadcastTransaction(transaction, sourcePeerId);

// Analyze propagation results
console.log('Transaction propagation time (ms):');
console.log(`  Median: ${propagationResults.median}ms`);
console.log(`  Mean: ${propagationResults.mean}ms`);
console.log(`  95th percentile: ${propagationResults.percentile95}ms`);
console.log(`  Maximum: ${propagationResults.max}ms`);
console.log(`  Coverage: ${propagationResults.coverage * 100}% of network`);

// Check for propagation issues
if (propagationResults.coverage < 0.98) {
  console.log('Warning: Transaction did not reach the entire network');
  console.log('Isolated node sets:', propagationResults.isolatedSets);
}

// Test impact of transaction size on propagation
const largeTransaction = new SimulatedTransaction({
  sender: 'WRF1Sender123456789',
  recipient: 'WRF1Recipient123456789',
  amount: 5.0,
  fee: 0.01,
  type: 'Computational',
  data: {
    // Large weather dataset
    weatherDataSet: Array(1000)
      .fill()
      .map((_, i) => ({
        location: 'Grid-' + i,
        temperature: 20 + Math.random() * 10,
        humidity: 50 + Math.random() * 30,
        pressure: 1000 + Math.random() * 30,
        timestamp: Date.now(),
      })),
  },
});

// Compare propagation times
const largeResults = await networkSim.broadcastTransaction(largeTransaction, sourcePeerId);

console.log('\nLarge transaction propagation time:');
console.log(`  Median: ${largeResults.median}ms`);
console.log(`  Mean: ${largeResults.mean}ms`);
console.log(`  Coverage: ${largeResults.coverage * 100}% of network`);

// Analyze network bottlenecks
const bottlenecks = networkSim.analyzeNetworkBottlenecks();
console.log('\nNetwork bottlenecks:');
bottlenecks.forEach(b => {
  console.log(`  Node ${b.nodeId}: ${b.bandwidth} Mbps, ${b.connections} connections`);
});
```

## Realistic Transaction Generation

Generate realistic transaction patterns for testing:

```javascript
const { TransactionGenerator } = require('../../utils/testing');

// Create a transaction generator
const generator = new TransactionGenerator({
  userCount: 100,
  transactionTypes: [
    { type: 'Regular', weight: 0.6 },
    { type: 'Computational', weight: 0.2 },
    { type: 'Governance', weight: 0.1 },
    { type: 'NFT', weight: 0.1 },
  ],
  feeRange: { min: 0.001, max: 0.1 },
  amountRange: { min: 0.1, max: 1000 },
  // Patterns like "high during business hours"
  timeDistribution: 'business_hours',
  // Some users are more active than others
  userActivityDistribution: 'zipf',
});

// Generate a set of transactions
const transactions = generator.generateTransactions(100);
console.log(`Generated ${transactions.length} transactions`);

// Analyze transaction distribution
const typeDistribution = {};
transactions.forEach(tx => {
  typeDistribution[tx.type] = (typeDistribution[tx.type] || 0) + 1;
});
console.log('Transaction type distribution:', typeDistribution);

// Calculate average fee and amount
const avgFee = transactions.reduce((sum, tx) => sum + tx.fee, 0) / transactions.length;
const avgAmount = transactions.reduce((sum, tx) => sum + tx.amount, 0) / transactions.length;
console.log(`Average fee: ${avgFee}`);
console.log(`Average amount: ${avgAmount}`);

// Generate a time series of transactions (e.g., for a 24-hour period)
const timeSeriesTransactions = generator.generateTimeSeries({
  duration: 24 * 60 * 60 * 1000, // 24 hours in ms
  averageRate: 2, // transactions per minute
  pattern: 'daily_cycle', // More during day, less at night
});

console.log(`Generated ${timeSeriesTransactions.length} transactions over 24 hours`);

// Analyze transaction timing
const hourlyDistribution = Array(24).fill(0);
timeSeriesTransactions.forEach(tx => {
  const hour = new Date(tx.timestamp).getHours();
  hourlyDistribution[hour]++;
});
console.log('Hourly transaction distribution:', hourlyDistribution);

// Generate transactions with specific data for weather-related operations
const weatherTransactions = generator.generateWeatherDataTransactions({
  count: 20,
  locations: ['Berlin', 'New York', 'Tokyo', 'Sydney', 'Rio'],
  dataTypes: ['temperature', 'humidity', 'pressure', 'windSpeed'],
});

console.log('Sample weather transaction:');
console.log(weatherTransactions[0]);
```

## Advanced Transaction Testing

### Testing Transaction Prioritization

```javascript
const { SimulatedTransaction, TransactionPoolSimulator } = require('../../utils/testing');

// Create a transaction pool
const txPool = new TransactionPoolSimulator({
  maxPoolSize: 1000,
  blockSize: 10,
});

// Generate transactions with different fees
const transactions = [];
for (let i = 0; i < 30; i++) {
  // Fee increases with index, but with some variation
  const fee = 0.001 + i * 0.001 + Math.random() * 0.0005;

  transactions.push(
    new SimulatedTransaction({
      sender: `WRF1Sender${i}`,
      recipient: `WRF1Recipient${i}`,
      amount: 1.0,
      fee,
      type: 'Regular',
    })
  );
}

// Add transactions to pool in random order
const shuffled = [...transactions];
for (let i = shuffled.length - 1; i > 0; i--) {
  const j = Math.floor(Math.random() * (i + 1));
  [shuffled[i], shuffled[j]] = [shuffled[j], shuffled[i]];
}

shuffled.forEach(tx => {
  tx.sign('privateKeySimulation');
  txPool.addTransaction(tx);
});

// Get transactions for three consecutive blocks
const block1 = txPool.getTransactionsForBlock();
const block2 = txPool.getTransactionsForBlock();
const block3 = txPool.getTransactionsForBlock();

// Check if transactions are properly prioritized by fee
console.log('Block 1 transactions (highest fees):');
block1.forEach(tx => console.log(`  Fee: ${tx.fee}`));

console.log('Block 2 transactions (medium fees):');
block2.forEach(tx => console.log(`  Fee: ${tx.fee}`));

console.log('Block 3 transactions (lowest fees):');
block3.forEach(tx => console.log(`  Fee: ${tx.fee}`));

// Verify high-fee transactions were included first
const allBlocks = [...block1, ...block2, ...block3];
const inCorrectOrder = allBlocks.every((tx, i) => i === 0 || tx.fee <= allBlocks[i - 1].fee);

console.log(`Transactions in correct fee priority order: ${inCorrectOrder}`);
```

### Stress Testing the Transaction Pool

```javascript
const {
  SimulatedTransaction,
  TransactionPoolSimulator,
  TransactionGenerator,
} = require('../../utils/testing');

// Create a transaction pool with limited size
const txPool = new TransactionPoolSimulator({
  maxPoolSize: 500,
  maxTransactionsPerSender: 5,
});

// Create a transaction generator for stress testing
const generator = new TransactionGenerator({ userCount: 200 });

// Stress test 1: Fill the pool to capacity
console.log('Filling transaction pool to capacity...');
let accepted = 0;
let rejected = 0;

for (let i = 0; i < 1000; i++) {
  const tx = generator.generateTransactions(1)[0];
  tx.sign('privateKeySimulation');

  if (txPool.addTransaction(tx)) {
    accepted++;
  } else {
    rejected++;
  }
}

console.log(`Added ${accepted} transactions, rejected ${rejected} transactions`);
console.log(`Pool size: ${txPool.getPoolSize()}`);

// Stress test 2: Add many transactions from the same sender
console.log('\nTesting sender limits...');
const singleSenderTxs = [];
for (let i = 0; i < 20; i++) {
  singleSenderTxs.push(
    new SimulatedTransaction({
      sender: 'WRF1SenderLimit',
      recipient: `WRF1Recipient${i}`,
      amount: 1.0,
      fee: 0.01 + i * 0.001, // Increasing fee
      type: 'Regular',
    })
  );
}

let senderAccepted = 0;
singleSenderTxs.forEach(tx => {
  tx.sign('privateKeySimulation');
  if (txPool.addTransaction(tx)) {
    senderAccepted++;
  }
});

console.log(
  `Accepted ${senderAccepted} of ${singleSenderTxs.length} transactions from single sender`
);

// Test replacing transactions with higher fee versions
console.log('\nTesting transaction replacement with higher fees...');
const originalTx = new SimulatedTransaction({
  sender: 'WRF1SenderReplace',
  recipient: 'WRF1RecipientReplace',
  amount: 5.0,
  fee: 0.01,
  type: 'Regular',
  nonce: 1, // Same nonce for replacement
});
originalTx.sign('privateKeySimulation');
txPool.addTransaction(originalTx);

// Create a replacement with higher fee
const replacementTx = new SimulatedTransaction({
  sender: 'WRF1SenderReplace',
  recipient: 'WRF1RecipientReplace',
  amount: 5.0,
  fee: 0.02, // Higher fee
  type: 'Regular',
  nonce: 1, // Same nonce for replacement
});
replacementTx.sign('privateKeySimulation');

const replaced = txPool.addTransaction(replacementTx);
console.log(`Transaction replaced: ${replaced}`);

// Verify the replacement
const poolTxs = txPool.getAllTransactions();
const replacementInPool = poolTxs.find(tx => tx.sender === 'WRF1SenderReplace' && tx.nonce === 1);

console.log('Transaction in pool:', replacementInPool);
console.log('Fee of transaction in pool:', replacementInPool.fee);
console.log('Is it the replacement:', replacementInPool.fee === 0.02);
```

These examples demonstrate how to use the transaction simulation utilities in the Wrfcoin Testing
Framework. You can adapt these patterns to test various transaction scenarios in your blockchain
applications.
