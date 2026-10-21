# Advanced JavaScript SDK Integration

Learn advanced patterns and features of the WRFCoin JavaScript SDK for building production-ready
applications.

## What You'll Learn

- Advanced client configuration and connection management
- Complex transaction patterns and batch operations
- Real-time data streaming and websocket integration
- Error handling strategies and retry mechanisms
- Performance optimization techniques
- Production deployment considerations

## Prerequisites

- Completed [Getting Started tutorials](../../getting-started/)
- Solid JavaScript/TypeScript knowledge
- Experience with async/await patterns
- Basic understanding of blockchain concepts

---

## 1. Advanced Client Configuration

### Production-Ready Setup

```javascript
// config/wrfcoin-config.js
const { WrfcoinClient } = require('@wrfcoin/sdk');

class WrfcoinManager {
  constructor(config = {}) {
    this.config = {
      network: process.env.NODE_ENV === 'production' ? 'mainnet' : 'testnet',
      apiUrl: process.env.WRFCOIN_API_URL,
      websocketUrl: process.env.WRFCOIN_WS_URL,
      retryConfig: {
        maxRetries: 3,
        backoffMultiplier: 2,
        initialDelay: 1000,
      },
      connectionPool: {
        maxConnections: 10,
        idleTimeout: 30000,
        connectionTimeout: 10000,
      },
      cache: {
        enabled: true,
        ttl: 300000, // 5 minutes
        maxSize: 1000,
      },
      ...config,
    };

    this.client = null;
    this.connectionPool = new Map();
    this.reconnectAttempts = 0;
    this.maxReconnectAttempts = 5;
  }

  async initialize() {
    console.log('🔌 Initializing WRFCoin client...');

    try {
      this.client = new WrfcoinClient({
        ...this.config,
        onConnection: this._handleConnection.bind(this),
        onDisconnection: this._handleDisconnection.bind(this),
        onError: this._handleError.bind(this),
      });

      // Test connection
      await this._validateConnection();

      // Setup event listeners
      this._setupEventListeners();

      console.log('✅ WRFCoin client initialized successfully');
      return this.client;
    } catch (error) {
      console.error('❌ Failed to initialize WRFCoin client:', error);
      throw error;
    }
  }

  async _validateConnection() {
    const startTime = Date.now();
    const networkInfo = await this.client.getNetworkInfo();
    const responseTime = Date.now() - startTime;

    console.log(`✅ Connected to ${networkInfo.name}`);
    console.log(`   - Response time: ${responseTime}ms`);
    console.log(`   - Block height: ${networkInfo.blockHeight}`);
    console.log(`   - Network status: ${networkInfo.status}`);

    if (responseTime > 5000) {
      console.warn('⚠️  High latency detected, consider switching endpoints');
    }
  }

  _setupEventListeners() {
    // Network status monitoring
    this.client.on('networkStatus', status => {
      console.log(`📡 Network status: ${status.health} (${status.latency}ms)`);

      if (status.health === 'degraded') {
        console.warn('⚠️  Network performance degraded');
        this._handleDegradedPerformance();
      }
    });

    // Transaction events
    this.client.on('transaction', tx => {
      console.log(`📝 Transaction: ${tx.hash} - ${tx.status}`);
    });

    // Data submission events
    this.client.on('dataSubmitted', submission => {
      console.log(`📊 Data submitted: ${submission.id} (${submission.type})`);
    });
  }

  _handleConnection() {
    console.log('🟢 Connected to WRFCoin network');
    this.reconnectAttempts = 0;
  }

  _handleDisconnection() {
    console.log('🔴 Disconnected from WRFCoin network');
    this._attemptReconnection();
  }

  _handleError(error) {
    console.error('❌ WRFCoin client error:', error);

    // Categorize errors and respond appropriately
    switch (error.type) {
      case 'NETWORK_ERROR':
        this._handleNetworkError(error);
        break;
      case 'API_ERROR':
        this._handleApiError(error);
        break;
      case 'VALIDATION_ERROR':
        this._handleValidationError(error);
        break;
      default:
        console.error('Unknown error type:', error);
    }
  }

  async _attemptReconnection() {
    if (this.reconnectAttempts >= this.maxReconnectAttempts) {
      console.error('❌ Max reconnection attempts reached');
      return;
    }

    this.reconnectAttempts++;
    const delay = Math.pow(2, this.reconnectAttempts) * 1000; // Exponential backoff

    console.log(
      `🔄 Attempting reconnection ${this.reconnectAttempts}/${this.maxReconnectAttempts} in ${delay}ms...`
    );

    setTimeout(async () => {
      try {
        await this.client.reconnect();
      } catch (error) {
        console.error('Reconnection failed:', error);
        this._attemptReconnection();
      }
    }, delay);
  }

  _handleDegradedPerformance() {
    // Implement fallback strategies
    console.log('🔄 Implementing performance optimizations...');

    // Reduce request frequency
    this.client.setRequestThrottle(2000); // 2 second delays

    // Switch to lighter operations
    this.client.setLightMode(true);

    // Consider endpoint switching
    this._considerEndpointSwitch();
  }

  async getHealthStatus() {
    return {
      connected: this.client?.isConnected() || false,
      networkStatus: await this.client?.getNetworkInfo(),
      reconnectAttempts: this.reconnectAttempts,
      connectionPool: this.connectionPool.size,
    };
  }
}

// Export singleton instance
module.exports = new WrfcoinManager();
```

### Environment-Specific Configuration

```javascript
// config/environments.js
const environments = {
  development: {
    network: 'testnet',
    apiUrl: 'https://testnet-api.wrfcoin.com',
    websocketUrl: 'wss://testnet-ws.wrfcoin.com',
    logLevel: 'debug',
    cache: { enabled: false },
    retries: { maxRetries: 1 },
  },

  staging: {
    network: 'testnet',
    apiUrl: 'https://staging-api.wrfcoin.com',
    websocketUrl: 'wss://staging-ws.wrfcoin.com',
    logLevel: 'info',
    cache: { enabled: true, ttl: 60000 },
    retries: { maxRetries: 3 },
  },

  production: {
    network: 'mainnet',
    apiUrl: 'https://api.wrfcoin.com',
    websocketUrl: 'wss://ws.wrfcoin.com',
    logLevel: 'warn',
    cache: { enabled: true, ttl: 300000 },
    retries: { maxRetries: 5 },
    monitoring: { enabled: true },
  },
};

module.exports = environments[process.env.NODE_ENV || 'development'];
```

---

## 2. Advanced Transaction Patterns

### Batch Transaction Processing

```javascript
// utils/transaction-batch.js
class TransactionBatchProcessor {
  constructor(client, options = {}) {
    this.client = client;
    this.batchSize = options.batchSize || 10;
    this.maxConcurrency = options.maxConcurrency || 5;
    this.retryAttempts = options.retryAttempts || 3;
    this.pendingBatches = new Map();
  }

  async processBatch(transactions) {
    console.log(`📦 Processing batch of ${transactions.length} transactions`);

    // Split into batches
    const batches = this._splitIntoBatches(transactions);
    const results = [];

    // Process batches with controlled concurrency
    for (let i = 0; i < batches.length; i += this.maxConcurrency) {
      const concurrentBatches = batches.slice(i, i + this.maxConcurrency);

      const batchPromises = concurrentBatches.map(batch =>
        this._processSingleBatch(batch, i + concurrentBatches.indexOf(batch))
      );

      const batchResults = await Promise.allSettled(batchPromises);
      results.push(...batchResults);
    }

    return this._processBatchResults(results);
  }

  async _processSingleBatch(batch, batchIndex) {
    const batchId = `batch_${Date.now()}_${batchIndex}`;
    this.pendingBatches.set(batchId, { status: 'processing', transactions: batch });

    try {
      console.log(`🔄 Processing batch ${batchIndex + 1} (${batch.length} transactions)`);

      // Prepare all transactions in the batch
      const preparedTxs = await Promise.all(batch.map(tx => this._prepareTransaction(tx)));

      // Submit batch to network
      const batchResult = await this.client.transaction.submitBatch(preparedTxs);

      // Monitor batch confirmation
      const confirmedResults = await this._monitorBatchConfirmation(batchResult.batchId);

      this.pendingBatches.set(batchId, { status: 'completed', results: confirmedResults });

      return confirmedResults;
    } catch (error) {
      console.error(`❌ Batch ${batchIndex + 1} failed:`, error);
      this.pendingBatches.set(batchId, { status: 'failed', error });

      // Retry failed transactions individually
      return await this._retryFailedTransactions(batch);
    }
  }

  async _prepareTransaction(tx) {
    // Estimate gas
    const gasEstimate = await this.client.transaction.estimateGas(tx);

    // Get current gas price
    const gasPrice = await this.client.blockchain.getOptimalGasPrice();

    return {
      ...tx,
      gasLimit: Math.ceil(gasEstimate * 1.2), // 20% buffer
      gasPrice: gasPrice,
      nonce: await this.client.wallet.getNonce(tx.from),
    };
  }

  async _monitorBatchConfirmation(batchId) {
    console.log(`⏳ Monitoring batch confirmation: ${batchId}`);

    let attempts = 0;
    const maxAttempts = 60; // 5 minutes max wait

    while (attempts < maxAttempts) {
      const status = await this.client.transaction.getBatchStatus(batchId);

      if (status.confirmed >= status.total * 0.8) {
        // 80% confirmation threshold
        console.log(`✅ Batch ${batchId} confirmed (${status.confirmed}/${status.total})`);
        return status.transactions;
      }

      if (status.failed > status.total * 0.2) {
        // Too many failures
        throw new Error(`Batch ${batchId} has too many failed transactions`);
      }

      attempts++;
      await new Promise(resolve => setTimeout(resolve, 5000)); // Wait 5 seconds
    }

    throw new Error(`Batch ${batchId} confirmation timeout`);
  }

  _splitIntoBatches(transactions) {
    const batches = [];
    for (let i = 0; i < transactions.length; i += this.batchSize) {
      batches.push(transactions.slice(i, i + this.batchSize));
    }
    return batches;
  }

  _processBatchResults(results) {
    const successful = results.filter(r => r.status === 'fulfilled').map(r => r.value);
    const failed = results.filter(r => r.status === 'rejected').map(r => r.reason);

    return {
      successful: successful.flat(),
      failed: failed,
      total: results.length,
      successRate: successful.length / results.length,
    };
  }

  getBatchStatus(batchId) {
    return this.pendingBatches.get(batchId) || { status: 'not_found' };
  }

  getAllBatches() {
    return Array.from(this.pendingBatches.entries()).map(([id, batch]) => ({
      id,
      ...batch,
    }));
  }
}

module.exports = TransactionBatchProcessor;
```

### Smart Transaction Queue

```javascript
// utils/transaction-queue.js
class SmartTransactionQueue {
  constructor(client) {
    this.client = client;
    this.queue = [];
    this.processing = false;
    this.maxQueueSize = 1000;
    this.priorityLevels = ['low', 'medium', 'high', 'urgent'];
  }

  async addTransaction(tx, priority = 'medium', options = {}) {
    if (this.queue.length >= this.maxQueueSize) {
      throw new Error('Transaction queue is full');
    }

    const queueItem = {
      id: `tx_${Date.now()}_${Math.random().toString(36).substr(2, 9)}`,
      transaction: tx,
      priority: this._validatePriority(priority),
      addedAt: Date.now(),
      retries: 0,
      maxRetries: options.maxRetries || 3,
      callback: options.callback,
      metadata: options.metadata || {},
    };

    this.queue.push(queueItem);
    this._sortQueue();

    console.log(`📋 Added transaction to queue: ${queueItem.id} (Priority: ${priority})`);

    if (!this.processing) {
      this._processQueue();
    }

    return queueItem.id;
  }

  async _processQueue() {
    if (this.processing || this.queue.length === 0) return;

    this.processing = true;
    console.log(`🔄 Processing transaction queue (${this.queue.length} items)`);

    while (this.queue.length > 0) {
      const item = this.queue.shift();

      try {
        console.log(`📤 Processing transaction: ${item.id}`);

        // Check network conditions
        const networkStatus = await this.client.getNetworkStatus();
        if (networkStatus.congestion > 0.8) {
          console.log('⏳ Network congested, delaying transaction...');
          await this._delay(5000); // Wait 5 seconds
        }

        // Process the transaction
        const result = await this._processTransaction(item);

        if (item.callback) {
          item.callback(null, result);
        }

        console.log(`✅ Transaction completed: ${item.id}`);
      } catch (error) {
        console.error(`❌ Transaction failed: ${item.id}`, error);

        if (item.retries < item.maxRetries) {
          item.retries++;
          console.log(`🔄 Retrying transaction: ${item.id} (${item.retries}/${item.maxRetries})`);

          // Add back to queue with delay
          setTimeout(
            () => {
              this.queue.unshift(item);
              this._sortQueue();
            },
            Math.pow(2, item.retries) * 1000
          ); // Exponential backoff
        } else {
          console.error(`💀 Transaction permanently failed: ${item.id}`);

          if (item.callback) {
            item.callback(error, null);
          }
        }
      }

      // Small delay between transactions
      await this._delay(100);
    }

    this.processing = false;
    console.log('✅ Transaction queue processing completed');
  }

  async _processTransaction(item) {
    const { transaction } = item;

    // Prepare transaction with latest data
    const preparedTx = await this.client.transaction.prepare({
      ...transaction,
      gasPrice: await this._getOptimalGasPrice(),
      nonce: await this.client.wallet.getNonce(transaction.from),
    });

    // Submit transaction
    const result = await this.client.transaction.submit(preparedTx);

    // Wait for confirmation
    const confirmation = await this.client.transaction.waitForConfirmation(
      result.hash,
      { timeout: 120000 } // 2 minutes
    );

    return {
      hash: result.hash,
      status: confirmation.status,
      blockNumber: confirmation.blockNumber,
      gasUsed: confirmation.gasUsed,
    };
  }

  async _getOptimalGasPrice() {
    const gasPrice = await this.client.blockchain.getGasPrice();
    const networkCongestion = await this.client.getNetworkCongestion();

    // Adjust gas price based on network conditions
    if (networkCongestion > 0.7) {
      return Math.ceil(gasPrice * 1.5); // 50% increase for high congestion
    } else if (networkCongestion < 0.3) {
      return Math.max(gasPrice * 0.8, gasPrice - 1000000000); // Reduce for low congestion
    }

    return gasPrice;
  }

  _validatePriority(priority) {
    return this.priorityLevels.includes(priority) ? priority : 'medium';
  }

  _sortQueue() {
    const priorityValues = { urgent: 4, high: 3, medium: 2, low: 1 };

    this.queue.sort((a, b) => {
      // First sort by priority
      const priorityDiff = priorityValues[b.priority] - priorityValues[a.priority];
      if (priorityDiff !== 0) return priorityDiff;

      // Then by age (older first)
      return a.addedAt - b.addedAt;
    });
  }

  _delay(ms) {
    return new Promise(resolve => setTimeout(resolve, ms));
  }

  getQueueStatus() {
    return {
      length: this.queue.length,
      processing: this.processing,
      byPriority: this.priorityLevels.reduce((acc, priority) => {
        acc[priority] = this.queue.filter(item => item.priority === priority).length;
        return acc;
      }, {}),
    };
  }

  clearQueue() {
    this.queue = [];
    console.log('🧹 Transaction queue cleared');
  }
}

module.exports = SmartTransactionQueue;
```

---

## 3. Real-time Data Streaming

### WebSocket Integration

```javascript
// services/realtime-data-stream.js
const EventEmitter = require('events');

class RealTimeDataStream extends EventEmitter {
  constructor(client, options = {}) {
    super();
    this.client = client;
    this.websocket = null;
    this.subscriptions = new Map();
    this.reconnectAttempts = 0;
    this.maxReconnectAttempts = options.maxReconnectAttempts || 10;
    this.heartbeatInterval = null;
    this.pingInterval = options.pingInterval || 30000; // 30 seconds
  }

  async connect() {
    try {
      console.log('🔌 Connecting to WRFCoin real-time data stream...');

      this.websocket = await this.client.createWebSocketConnection({
        onOpen: this._handleOpen.bind(this),
        onMessage: this._handleMessage.bind(this),
        onClose: this._handleClose.bind(this),
        onError: this._handleError.bind(this),
      });
    } catch (error) {
      console.error('❌ WebSocket connection failed:', error);
      throw error;
    }
  }

  _handleOpen() {
    console.log('✅ WebSocket connected');
    this.reconnectAttempts = 0;
    this._startHeartbeat();
    this._resubscribeAll();
    this.emit('connected');
  }

  _handleMessage(message) {
    try {
      const data = JSON.parse(message);

      switch (data.type) {
        case 'weather_data':
          this._handleWeatherData(data);
          break;
        case 'transaction_update':
          this._handleTransactionUpdate(data);
          break;
        case 'governance_proposal':
          this._handleGovernanceProposal(data);
          break;
        case 'network_status':
          this._handleNetworkStatus(data);
          break;
        case 'pong':
          this._handlePong(data);
          break;
        default:
          console.log('📨 Unknown message type:', data.type);
      }
    } catch (error) {
      console.error('❌ Error parsing WebSocket message:', error);
    }
  }

  _handleClose(code, reason) {
    console.log(`🔴 WebSocket disconnected: ${code} - ${reason}`);
    this._stopHeartbeat();
    this.emit('disconnected', { code, reason });
    this._attemptReconnection();
  }

  _handleError(error) {
    console.error('❌ WebSocket error:', error);
    this.emit('error', error);
  }

  // Subscription management
  async subscribeToWeatherData(location, callback) {
    const subscriptionId = `weather_${location.lat}_${location.lng}`;

    this.subscriptions.set(subscriptionId, {
      type: 'weather_data',
      location,
      callback,
    });

    if (this.websocket?.readyState === 1) {
      // WebSocket.OPEN
      await this._sendSubscription(subscriptionId);
    }

    console.log(`📡 Subscribed to weather data for ${location.lat}, ${location.lng}`);
    return subscriptionId;
  }

  async subscribeToTransactions(address, callback) {
    const subscriptionId = `transactions_${address}`;

    this.subscriptions.set(subscriptionId, {
      type: 'transactions',
      address,
      callback,
    });

    if (this.websocket?.readyState === 1) {
      await this._sendSubscription(subscriptionId);
    }

    console.log(`📡 Subscribed to transactions for ${address}`);
    return subscriptionId;
  }

  async subscribeToGovernance(callback) {
    const subscriptionId = 'governance_proposals';

    this.subscriptions.set(subscriptionId, {
      type: 'governance',
      callback,
    });

    if (this.websocket?.readyState === 1) {
      await this._sendSubscription(subscriptionId);
    }

    console.log('📡 Subscribed to governance proposals');
    return subscriptionId;
  }

  async unsubscribe(subscriptionId) {
    if (this.subscriptions.has(subscriptionId)) {
      this.subscriptions.delete(subscriptionId);

      if (this.websocket?.readyState === 1) {
        this.websocket.send(
          JSON.stringify({
            type: 'unsubscribe',
            subscriptionId,
          })
        );
      }

      console.log(`📡 Unsubscribed from ${subscriptionId}`);
    }
  }

  async _sendSubscription(subscriptionId) {
    const subscription = this.subscriptions.get(subscriptionId);

    this.websocket.send(
      JSON.stringify({
        type: 'subscribe',
        subscriptionId,
        subscription,
      })
    );
  }

  async _resubscribeAll() {
    console.log('🔄 Resubscribing to all data streams...');

    for (const [subscriptionId] of this.subscriptions) {
      await this._sendSubscription(subscriptionId);
    }
  }

  // Data handlers
  _handleWeatherData(data) {
    const location = `${data.location.lat}_${data.location.lng}`;
    const subscriptionId = `weather_${location}`;
    const subscription = this.subscriptions.get(subscriptionId);

    if (subscription?.callback) {
      subscription.callback(data.payload);
    }

    this.emit('weatherData', data.payload);
  }

  _handleTransactionUpdate(data) {
    const subscriptionId = `transactions_${data.address}`;
    const subscription = this.subscriptions.get(subscriptionId);

    if (subscription?.callback) {
      subscription.callback(data.payload);
    }

    this.emit('transactionUpdate', data.payload);
  }

  _handleGovernanceProposal(data) {
    const subscription = this.subscriptions.get('governance_proposals');

    if (subscription?.callback) {
      subscription.callback(data.payload);
    }

    this.emit('governanceProposal', data.payload);
  }

  _handleNetworkStatus(data) {
    this.emit('networkStatus', data.payload);
  }

  // Heartbeat mechanism
  _startHeartbeat() {
    this.heartbeatInterval = setInterval(() => {
      if (this.websocket?.readyState === 1) {
        this.websocket.send(JSON.stringify({ type: 'ping' }));
      }
    }, this.pingInterval);
  }

  _stopHeartbeat() {
    if (this.heartbeatInterval) {
      clearInterval(this.heartbeatInterval);
      this.heartbeatInterval = null;
    }
  }

  _handlePong(data) {
    // Connection is alive
    console.log('💓 Heartbeat received');
  }

  // Reconnection logic
  async _attemptReconnection() {
    if (this.reconnectAttempts >= this.maxReconnectAttempts) {
      console.error('❌ Max reconnection attempts reached');
      this.emit('maxReconnectAttemptsReached');
      return;
    }

    this.reconnectAttempts++;
    const delay = Math.min(Math.pow(2, this.reconnectAttempts) * 1000, 30000); // Max 30s

    console.log(
      `🔄 Attempting reconnection ${this.reconnectAttempts}/${this.maxReconnectAttempts} in ${delay}ms...`
    );

    setTimeout(async () => {
      try {
        await this.connect();
      } catch (error) {
        console.error('Reconnection failed:', error);
      }
    }, delay);
  }

  disconnect() {
    if (this.websocket) {
      this._stopHeartbeat();
      this.websocket.close();
      this.websocket = null;
    }
  }

  getConnectionStatus() {
    return {
      connected: this.websocket?.readyState === 1,
      subscriptions: this.subscriptions.size,
      reconnectAttempts: this.reconnectAttempts,
    };
  }
}

module.exports = RealTimeDataStream;
```

### Live Data Dashboard

```javascript
// examples/realtime-dashboard.js
const WrfcoinManager = require('../config/wrfcoin-config');
const RealTimeDataStream = require('../services/realtime-data-stream');

class LiveDataDashboard {
  constructor() {
    this.dataStream = null;
    this.client = null;
    this.weatherStations = new Map();
    this.transactionHistory = [];
    this.governanceUpdates = [];
  }

  async initialize() {
    console.log('📊 Initializing Live Data Dashboard\n');

    try {
      // Initialize WRFCoin client
      this.client = await WrfcoinManager.initialize();

      // Create real-time data stream
      this.dataStream = new RealTimeDataStream(this.client);

      // Setup event listeners
      this._setupEventListeners();

      // Connect to real-time stream
      await this.dataStream.connect();

      // Subscribe to various data feeds
      await this._setupSubscriptions();

      console.log('✅ Live Data Dashboard initialized successfully\n');

      // Start dashboard display
      this._startDashboardDisplay();
    } catch (error) {
      console.error('❌ Dashboard initialization failed:', error);
      throw error;
    }
  }

  _setupEventListeners() {
    this.dataStream.on('connected', () => {
      console.log('🟢 Real-time data stream connected');
    });

    this.dataStream.on('disconnected', info => {
      console.log('🔴 Real-time data stream disconnected:', info);
    });

    this.dataStream.on('error', error => {
      console.error('❌ Stream error:', error);
    });

    this.dataStream.on('weatherData', data => {
      this._updateWeatherData(data);
    });

    this.dataStream.on('transactionUpdate', tx => {
      this._updateTransactionHistory(tx);
    });

    this.dataStream.on('governanceProposal', proposal => {
      this._updateGovernanceData(proposal);
    });

    this.dataStream.on('networkStatus', status => {
      this._updateNetworkStatus(status);
    });
  }

  async _setupSubscriptions() {
    console.log('📡 Setting up data subscriptions...');

    // Weather data from major cities
    const majorCities = [
      { name: 'New York', lat: 40.7128, lng: -74.006 },
      { name: 'London', lat: 51.5074, lng: -0.1278 },
      { name: 'Tokyo', lat: 35.6762, lng: 139.6503 },
      { name: 'Sydney', lat: -33.8688, lng: 151.2093 },
      { name: 'São Paulo', lat: -23.5505, lng: -46.6333 },
    ];

    for (const city of majorCities) {
      await this.dataStream.subscribeToWeatherData({ lat: city.lat, lng: city.lng }, data =>
        this._handleCityWeatherUpdate(city.name, data)
      );
    }

    // Subscribe to governance proposals
    await this.dataStream.subscribeToGovernance(proposal => {
      console.log(`🏛️  New governance proposal: ${proposal.title}`);
    });

    console.log('✅ Subscriptions established\n');
  }

  _handleCityWeatherUpdate(cityName, data) {
    this.weatherStations.set(cityName, {
      ...data,
      lastUpdate: new Date().toISOString(),
      city: cityName,
    });
  }

  _updateWeatherData(data) {
    // Update local weather data cache
    console.log(`🌡️  Weather update: ${data.temperature}°C, ${data.humidity}% humidity`);
  }

  _updateTransactionHistory(tx) {
    this.transactionHistory.unshift({
      ...tx,
      timestamp: new Date().toISOString(),
    });

    // Keep only last 100 transactions
    if (this.transactionHistory.length > 100) {
      this.transactionHistory = this.transactionHistory.slice(0, 100);
    }

    console.log(`💸 New transaction: ${tx.hash} (${tx.type})`);
  }

  _updateGovernanceData(proposal) {
    this.governanceUpdates.unshift({
      ...proposal,
      timestamp: new Date().toISOString(),
    });

    // Keep only last 50 updates
    if (this.governanceUpdates.length > 50) {
      this.governanceUpdates = this.governanceUpdates.slice(0, 50);
    }
  }

  _updateNetworkStatus(status) {
    console.log(`📡 Network: ${status.health} (${status.blockHeight} blocks, ${status.tps} TPS)`);
  }

  _startDashboardDisplay() {
    // Display dashboard every 5 seconds
    setInterval(() => {
      this._displayDashboard();
    }, 5000);
  }

  _displayDashboard() {
    console.clear();
    console.log('🌍 WRFCoin Live Data Dashboard');
    console.log('='.repeat(50));

    // Network status
    console.log('\n📡 Network Status:');
    console.log(`Connected: ${this.dataStream.getConnectionStatus().connected ? '🟢' : '🔴'}`);
    console.log(`Subscriptions: ${this.dataStream.getConnectionStatus().subscriptions}`);

    // Weather data
    console.log('\n🌡️  Latest Weather Data:');
    for (const [city, data] of this.weatherStations.entries()) {
      console.log(`${city}: ${data.temperature}°C, ${data.humidity}% humidity`);
    }

    // Recent transactions
    console.log('\n💸 Recent Transactions:');
    this.transactionHistory.slice(0, 5).forEach(tx => {
      console.log(
        `${tx.hash.substring(0, 10)}... - ${tx.type} (${new Date(tx.timestamp).toLocaleTimeString()})`
      );
    });

    // Governance updates
    console.log('\n🏛️  Governance Updates:');
    this.governanceUpdates.slice(0, 3).forEach(update => {
      console.log(
        `${update.title} - ${update.status} (${new Date(update.timestamp).toLocaleTimeString()})`
      );
    });

    console.log('\n' + '='.repeat(50));
    console.log('Press Ctrl+C to exit');
  }

  async shutdown() {
    console.log('\n🛑 Shutting down dashboard...');

    if (this.dataStream) {
      this.dataStream.disconnect();
    }

    console.log('✅ Dashboard shut down gracefully');
  }
}

// Usage example
const dashboard = new LiveDataDashboard();

// Handle graceful shutdown
process.on('SIGINT', async () => {
  await dashboard.shutdown();
  process.exit(0);
});

// Start the dashboard
dashboard.initialize().catch(console.error);
```

---

## 4. Error Handling and Resilience

### Comprehensive Error Handler

```javascript
// utils/error-handler.js
class WrfcoinErrorHandler {
  constructor(options = {}) {
    this.retryConfig = {
      maxRetries: options.maxRetries || 3,
      baseDelay: options.baseDelay || 1000,
      maxDelay: options.maxDelay || 30000,
      backoffMultiplier: options.backoffMultiplier || 2,
    };

    this.circuitBreaker = {
      enabled: options.circuitBreaker?.enabled || true,
      failureThreshold: options.circuitBreaker?.failureThreshold || 5,
      resetTimeout: options.circuitBreaker?.resetTimeout || 60000,
      states: new Map(), // endpoint -> state
    };

    this.errorMetrics = {
      totalErrors: 0,
      errorsByType: new Map(),
      errorsByEndpoint: new Map(),
      lastError: null,
    };
  }

  async executeWithRetry(operation, context = {}) {
    const startTime = Date.now();
    let lastError;

    for (let attempt = 1; attempt <= this.retryConfig.maxRetries + 1; attempt++) {
      try {
        // Check circuit breaker
        if (this._isCircuitOpen(context.endpoint)) {
          throw new Error(`Circuit breaker is OPEN for ${context.endpoint}`);
        }

        const result = await operation();

        // Reset circuit breaker on success
        if (context.endpoint) {
          this._resetCircuitBreaker(context.endpoint);
        }

        const duration = Date.now() - startTime;
        console.log(`✅ Operation succeeded after ${attempt} attempt(s) in ${duration}ms`);

        return result;
      } catch (error) {
        lastError = error;
        this._recordError(error, context);

        if (attempt <= this.retryConfig.maxRetries) {
          const isRetryable = this._isRetryableError(error);

          if (isRetryable) {
            const delay = this._calculateDelay(attempt);
            console.log(
              `🔄 Retry ${attempt}/${this.retryConfig.maxRetries} after ${delay}ms: ${error.message}`
            );

            await this._delay(delay);
            continue;
          } else {
            console.log(`❌ Non-retryable error: ${error.message}`);
            break;
          }
        }
      }
    }

    // Update circuit breaker on final failure
    if (context.endpoint) {
      this._recordCircuitBreakerFailure(context.endpoint);
    }

    const duration = Date.now() - startTime;
    console.error(
      `❌ Operation failed after ${this.retryConfig.maxRetries + 1} attempts in ${duration}ms`
    );

    throw this._enhanceError(lastError, context);
  }

  _isRetryableError(error) {
    const retryableErrors = [
      'NETWORK_ERROR',
      'TIMEOUT_ERROR',
      'RATE_LIMITED',
      'SERVER_ERROR',
      'TEMPORARY_UNAVAILABLE',
      'CONNECTION_RESET',
      'ETIMEDOUT',
      'ECONNRESET',
      'ENOTFOUND',
    ];

    return retryableErrors.some(
      type =>
        error.code === type ||
        error.message.includes(type) ||
        (error.status >= 500 && error.status < 600) // 5xx server errors
    );
  }

  _calculateDelay(attempt) {
    const exponentialDelay =
      this.retryConfig.baseDelay * Math.pow(this.retryConfig.backoffMultiplier, attempt - 1);

    // Add jitter to prevent thundering herd
    const jitter = Math.random() * 0.3 * exponentialDelay;

    return Math.min(exponentialDelay + jitter, this.retryConfig.maxDelay);
  }

  _recordError(error, context) {
    this.errorMetrics.totalErrors++;
    this.errorMetrics.lastError = {
      ...error,
      timestamp: new Date().toISOString(),
      context,
    };

    // Record by error type
    const errorType = error.code || error.name || 'Unknown';
    this.errorMetrics.errorsByType.set(
      errorType,
      (this.errorMetrics.errorsByType.get(errorType) || 0) + 1
    );

    // Record by endpoint
    if (context.endpoint) {
      this.errorMetrics.errorsByEndpoint.set(
        context.endpoint,
        (this.errorMetrics.errorsByEndpoint.get(context.endpoint) || 0) + 1
      );
    }
  }

  _enhanceError(error, context) {
    return {
      ...error,
      originalMessage: error.message,
      message: `${error.message} (Context: ${JSON.stringify(context)})`,
      timestamp: new Date().toISOString(),
      context,
      suggestions: this._generateSuggestions(error, context),
    };
  }

  _generateSuggestions(error, context) {
    const suggestions = [];

    switch (error.code) {
      case 'NETWORK_ERROR':
        suggestions.push('Check your internet connection');
        suggestions.push('Try switching to a different network endpoint');
        break;
      case 'INSUFFICIENT_BALANCE':
        suggestions.push('Check your wallet balance');
        suggestions.push('Ensure you have enough tokens for transaction + fees');
        break;
      case 'GAS_TOO_LOW':
        suggestions.push('Increase the gas price');
        suggestions.push('Wait for network congestion to decrease');
        break;
      case 'INVALID_ADDRESS':
        suggestions.push('Verify the address format');
        suggestions.push('Ensure the address is for the correct network');
        break;
      default:
        suggestions.push('Check the WRFCoin documentation');
        suggestions.push('Contact support if the issue persists');
    }

    return suggestions;
  }

  // Circuit Breaker Implementation
  _isCircuitOpen(endpoint) {
    if (!this.circuitBreaker.enabled || !endpoint) return false;

    const state = this.circuitBreaker.states.get(endpoint);
    if (!state) return false;

    if (state.status === 'OPEN') {
      const timeSinceOpen = Date.now() - state.lastFailure;
      if (timeSinceOpen >= this.circuitBreaker.resetTimeout) {
        // Move to HALF_OPEN state
        state.status = 'HALF_OPEN';
        console.log(`🔄 Circuit breaker for ${endpoint} moved to HALF_OPEN`);
        return false;
      }
      return true;
    }

    return false;
  }

  _recordCircuitBreakerFailure(endpoint) {
    if (!this.circuitBreaker.enabled) return;

    const state = this.circuitBreaker.states.get(endpoint) || {
      failures: 0,
      status: 'CLOSED',
      lastFailure: null,
    };

    state.failures++;
    state.lastFailure = Date.now();

    if (state.failures >= this.circuitBreaker.failureThreshold) {
      state.status = 'OPEN';
      console.log(`🚨 Circuit breaker OPENED for ${endpoint} after ${state.failures} failures`);
    }

    this.circuitBreaker.states.set(endpoint, state);
  }

  _resetCircuitBreaker(endpoint) {
    if (!this.circuitBreaker.enabled) return;

    const state = this.circuitBreaker.states.get(endpoint);
    if (state) {
      state.failures = 0;
      state.status = 'CLOSED';
      this.circuitBreaker.states.set(endpoint, state);
    }
  }

  _delay(ms) {
    return new Promise(resolve => setTimeout(resolve, ms));
  }

  getErrorMetrics() {
    return {
      ...this.errorMetrics,
      errorsByType: Object.fromEntries(this.errorMetrics.errorsByType),
      errorsByEndpoint: Object.fromEntries(this.errorMetrics.errorsByEndpoint),
      circuitBreakerStates: Object.fromEntries(this.circuitBreaker.states),
    };
  }

  resetMetrics() {
    this.errorMetrics = {
      totalErrors: 0,
      errorsByType: new Map(),
      errorsByEndpoint: new Map(),
      lastError: null,
    };

    this.circuitBreaker.states.clear();

    console.log('📊 Error metrics reset');
  }
}

module.exports = WrfcoinErrorHandler;
```

---

## What You've Learned

✅ **Advanced Configuration** - Production-ready client setup with connection pooling  
✅ **Batch Processing** - Efficient handling of multiple transactions  
✅ **Smart Queuing** - Priority-based transaction queue management  
✅ **Real-time Streaming** - WebSocket integration for live data  
✅ **Error Resilience** - Comprehensive error handling with circuit breakers  
✅ **Performance Optimization** - Network condition awareness and adaptive behavior

## Next Steps

1. **[Python SDK Advanced](./02-python-sdk-advanced.md)** - Advanced Python patterns
2. **[Smart Contract Integration](../smart-contracts/)** - Contract interaction patterns
3. **[Performance Optimization](../optimization/)** - Advanced performance techniques
4. **[Production Deployment](../deployment/)** - Deploy to production environments

---

**🎉 Congratulations!** You've mastered advanced JavaScript SDK patterns for WRFCoin. You're now
ready to build robust, production-ready climate applications!
