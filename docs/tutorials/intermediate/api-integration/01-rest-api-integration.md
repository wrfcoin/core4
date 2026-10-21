# REST API Integration Guide

Comprehensive guide to integrating with WRFCoin's REST API for building applications, dashboards,
and services.

## What You'll Learn

- WRFCoin REST API architecture and endpoints
- Authentication and rate limiting strategies
- Data retrieval and submission patterns
- Real-time updates with webhooks
- Error handling and retry mechanisms
- Performance optimization techniques

## Prerequisites

- Completed [SDK Integration tutorials](../sdk-integration/)
- Experience with REST APIs and HTTP clients
- Understanding of JSON data formats
- Basic knowledge of authentication methods

---

## 1. API Overview and Authentication

### API Base Configuration

```javascript
// config/api-client.js
const axios = require('axios');
const crypto = require('crypto');

class WRFCoinAPIClient {
  constructor(config = {}) {
    this.config = {
      baseURL: config.baseURL || 'https://api.wrfcoin.network',
      apiKey: config.apiKey || process.env.WRFCOIN_API_KEY,
      apiSecret: config.apiSecret || process.env.WRFCOIN_API_SECRET,
      network: config.network || 'mainnet', // or 'testnet'
      timeout: config.timeout || 30000,
      retryAttempts: config.retryAttempts || 3,
      retryDelay: config.retryDelay || 1000,
      ...config,
    };

    this.client = this._createHttpClient();
    this.rateLimiter = this._createRateLimiter();
  }

  _createHttpClient() {
    const client = axios.create({
      baseURL: this.config.baseURL,
      timeout: this.config.timeout,
      headers: {
        'Content-Type': 'application/json',
        'User-Agent': 'WRFCoin-Client/1.0.0',
        'X-API-Version': 'v1',
        'X-Network': this.config.network,
      },
    });

    // Request interceptor for authentication
    client.interceptors.request.use(
      config => this._signRequest(config),
      error => Promise.reject(error)
    );

    // Response interceptor for error handling and retries
    client.interceptors.response.use(
      response => this._handleResponse(response),
      error => this._handleError(error)
    );

    return client;
  }

  _signRequest(config) {
    const timestamp = Date.now().toString();
    const method = config.method.toUpperCase();
    const url = config.url;
    const body = config.data ? JSON.stringify(config.data) : '';

    // Create signature string
    const signatureString = `${timestamp}${method}${url}${body}`;
    const signature = crypto
      .createHmac('sha256', this.config.apiSecret)
      .update(signatureString)
      .digest('hex');

    // Add authentication headers
    config.headers['X-API-Key'] = this.config.apiKey;
    config.headers['X-Timestamp'] = timestamp;
    config.headers['X-Signature'] = signature;

    return config;
  }

  _handleResponse(response) {
    // Log successful requests for monitoring
    console.log(
      `✅ ${response.config.method.toUpperCase()} ${response.config.url} - ${response.status}`
    );

    return {
      data: response.data,
      status: response.status,
      headers: response.headers,
      rateLimit: {
        limit: parseInt(response.headers['x-ratelimit-limit']),
        remaining: parseInt(response.headers['x-ratelimit-remaining']),
        reset: parseInt(response.headers['x-ratelimit-reset']),
      },
    };
  }

  async _handleError(error) {
    if (error.response) {
      // Server responded with error status
      const { status, data } = error.response;

      switch (status) {
        case 429: // Rate limited
          const retryAfter = parseInt(error.response.headers['retry-after']) * 1000;
          console.warn(`⚠️  Rate limited. Retrying after ${retryAfter}ms`);
          await this._delay(retryAfter);
          return this.client.request(error.config);

        case 401: // Unauthorized
          console.error('❌ Authentication failed. Check API credentials.');
          throw new Error('Authentication failed');

        case 403: // Forbidden
          console.error('❌ Access forbidden. Check API permissions.');
          throw new Error('Access forbidden');

        case 500: // Server error
          console.error('❌ Server error. Retrying...');
          return this._retryRequest(error.config);

        default:
          console.error(`❌ API Error ${status}:`, data.message || data.error);
          throw new Error(data.message || `API Error: ${status}`);
      }
    } else if (error.request) {
      // Network error
      console.error('❌ Network error:', error.message);
      return this._retryRequest(error.config);
    } else {
      // Configuration error
      console.error('❌ Request setup error:', error.message);
      throw error;
    }
  }

  async _retryRequest(config, attemptNumber = 1) {
    if (attemptNumber > this.config.retryAttempts) {
      throw new Error(`Request failed after ${this.config.retryAttempts} attempts`);
    }

    const delay = this.config.retryDelay * Math.pow(2, attemptNumber - 1);
    console.log(`🔄 Retrying request (attempt ${attemptNumber}) after ${delay}ms`);

    await this._delay(delay);

    try {
      return await this.client.request(config);
    } catch (error) {
      return this._retryRequest(config, attemptNumber + 1);
    }
  }

  _delay(ms) {
    return new Promise(resolve => setTimeout(resolve, ms));
  }

  _createRateLimiter() {
    return {
      requests: [],
      maxRequests: 100,
      windowMs: 60000, // 1 minute

      canMakeRequest() {
        const now = Date.now();
        this.requests = this.requests.filter(time => now - time < this.windowMs);
        return this.requests.length < this.maxRequests;
      },

      recordRequest() {
        this.requests.push(Date.now());
      },
    };
  }
}

module.exports = WRFCoinAPIClient;
```

---

## 2. Core API Operations

### Blockchain Data Access

```javascript
// lib/blockchain-api.js
class BlockchainAPI extends WRFCoinAPIClient {
  constructor(config) {
    super(config);
  }

  // Get network information
  async getNetworkInfo() {
    try {
      const response = await this.client.get('/v1/network/info');
      return response.data;
    } catch (error) {
      console.error('Failed to get network info:', error.message);
      throw error;
    }
  }

  // Get block information
  async getBlock(blockNumber = 'latest') {
    try {
      const response = await this.client.get(`/v1/blocks/${blockNumber}`);
      return response.data;
    } catch (error) {
      console.error(`Failed to get block ${blockNumber}:`, error.message);
      throw error;
    }
  }

  // Get transaction details
  async getTransaction(txHash) {
    try {
      const response = await this.client.get(`/v1/transactions/${txHash}`);
      return response.data;
    } catch (error) {
      console.error(`Failed to get transaction ${txHash}:`, error.message);
      throw error;
    }
  }

  // Get address information
  async getAddress(address) {
    try {
      const response = await this.client.get(`/v1/addresses/${address}`);
      return response.data;
    } catch (error) {
      console.error(`Failed to get address ${address}:`, error.message);
      throw error;
    }
  }

  // Get address transactions
  async getAddressTransactions(address, options = {}) {
    try {
      const params = new URLSearchParams({
        page: options.page || 1,
        limit: options.limit || 20,
        sort: options.sort || 'desc',
        ...options.filters,
      });

      const response = await this.client.get(`/v1/addresses/${address}/transactions?${params}`);
      return response.data;
    } catch (error) {
      console.error(`Failed to get transactions for ${address}:`, error.message);
      throw error;
    }
  }

  // Get token balance
  async getTokenBalance(address, tokenAddress = null) {
    try {
      const url = tokenAddress
        ? `/v1/addresses/${address}/tokens/${tokenAddress}`
        : `/v1/addresses/${address}/balance`;

      const response = await this.client.get(url);
      return response.data;
    } catch (error) {
      console.error(`Failed to get token balance for ${address}:`, error.message);
      throw error;
    }
  }

  // Search transactions
  async searchTransactions(query, options = {}) {
    try {
      const params = new URLSearchParams({
        q: query,
        page: options.page || 1,
        limit: options.limit || 20,
        ...options,
      });

      const response = await this.client.get(`/v1/search/transactions?${params}`);
      return response.data;
    } catch (error) {
      console.error('Failed to search transactions:', error.message);
      throw error;
    }
  }
}
```

### Climate Data API

```javascript
// lib/climate-api.js
class ClimateAPI extends WRFCoinAPIClient {
  constructor(config) {
    super(config);
  }

  // Submit climate data
  async submitClimateData(data) {
    try {
      const payload = {
        stationId: data.stationId,
        timestamp: data.timestamp || new Date().toISOString(),
        location: {
          latitude: data.latitude,
          longitude: data.longitude,
          elevation: data.elevation,
        },
        measurements: {
          temperature: data.temperature,
          humidity: data.humidity,
          pressure: data.pressure,
          windSpeed: data.windSpeed,
          windDirection: data.windDirection,
          precipitation: data.precipitation,
          ...data.additionalMeasurements,
        },
        metadata: {
          deviceModel: data.deviceModel,
          firmwareVersion: data.firmwareVersion,
          calibrationDate: data.calibrationDate,
          accuracy: data.accuracy,
        },
        signature: data.signature,
      };

      const response = await this.client.post('/v1/climate/data', payload);
      return response.data;
    } catch (error) {
      console.error('Failed to submit climate data:', error.message);
      throw error;
    }
  }

  // Get climate data
  async getClimateData(options = {}) {
    try {
      const params = new URLSearchParams({
        startDate: options.startDate,
        endDate: options.endDate,
        latitude: options.latitude,
        longitude: options.longitude,
        radius: options.radius || 10, // km
        dataTypes: options.dataTypes?.join(','),
        aggregation: options.aggregation || 'raw',
        page: options.page || 1,
        limit: options.limit || 100,
      });

      const response = await this.client.get(`/v1/climate/data?${params}`);
      return response.data;
    } catch (error) {
      console.error('Failed to get climate data:', error.message);
      throw error;
    }
  }

  // Get weather stations
  async getWeatherStations(options = {}) {
    try {
      const params = new URLSearchParams({
        latitude: options.latitude,
        longitude: options.longitude,
        radius: options.radius || 50,
        status: options.status || 'active',
        page: options.page || 1,
        limit: options.limit || 50,
      });

      const response = await this.client.get(`/v1/climate/stations?${params}`);
      return response.data;
    } catch (error) {
      console.error('Failed to get weather stations:', error.message);
      throw error;
    }
  }

  // Register weather station
  async registerWeatherStation(stationData) {
    try {
      const payload = {
        name: stationData.name,
        description: stationData.description,
        location: {
          latitude: stationData.latitude,
          longitude: stationData.longitude,
          elevation: stationData.elevation,
          address: stationData.address,
        },
        deviceInfo: {
          model: stationData.deviceModel,
          serialNumber: stationData.serialNumber,
          firmwareVersion: stationData.firmwareVersion,
          manufacturer: stationData.manufacturer,
        },
        capabilities: stationData.capabilities,
        contactInfo: stationData.contactInfo,
        operatingSchedule: stationData.operatingSchedule,
      };

      const response = await this.client.post('/v1/climate/stations', payload);
      return response.data;
    } catch (error) {
      console.error('Failed to register weather station:', error.message);
      throw error;
    }
  }

  // Get validation status
  async getValidationStatus(dataId) {
    try {
      const response = await this.client.get(`/v1/climate/data/${dataId}/validation`);
      return response.data;
    } catch (error) {
      console.error(`Failed to get validation status for data ${dataId}:`, error.message);
      throw error;
    }
  }

  // Request data validation
  async requestValidation(dataId) {
    try {
      const response = await this.client.post(`/v1/climate/data/${dataId}/validate`);
      return response.data;
    } catch (error) {
      console.error(`Failed to request validation for data ${dataId}:`, error.message);
      throw error;
    }
  }
}
```

### Token and DeFi API

```javascript
// lib/defi-api.js
class DeFiAPI extends WRFCoinAPIClient {
  constructor(config) {
    super(config);
  }

  // Get token information
  async getTokenInfo(tokenAddress) {
    try {
      const response = await this.client.get(`/v1/tokens/${tokenAddress}`);
      return response.data;
    } catch (error) {
      console.error(`Failed to get token info for ${tokenAddress}:`, error.message);
      throw error;
    }
  }

  // Get token holders
  async getTokenHolders(tokenAddress, options = {}) {
    try {
      const params = new URLSearchParams({
        page: options.page || 1,
        limit: options.limit || 50,
        minBalance: options.minBalance || 0,
      });

      const response = await this.client.get(`/v1/tokens/${tokenAddress}/holders?${params}`);
      return response.data;
    } catch (error) {
      console.error(`Failed to get token holders for ${tokenAddress}:`, error.message);
      throw error;
    }
  }

  // Get staking information
  async getStakingInfo(address = null) {
    try {
      const url = address ? `/v1/staking/${address}` : '/v1/staking/info';
      const response = await this.client.get(url);
      return response.data;
    } catch (error) {
      console.error('Failed to get staking info:', error.message);
      throw error;
    }
  }

  // Get governance proposals
  async getGovernanceProposals(options = {}) {
    try {
      const params = new URLSearchParams({
        status: options.status || 'all',
        page: options.page || 1,
        limit: options.limit || 20,
        sort: options.sort || 'newest',
      });

      const response = await this.client.get(`/v1/governance/proposals?${params}`);
      return response.data;
    } catch (error) {
      console.error('Failed to get governance proposals:', error.message);
      throw error;
    }
  }

  // Get proposal details
  async getProposal(proposalId) {
    try {
      const response = await this.client.get(`/v1/governance/proposals/${proposalId}`);
      return response.data;
    } catch (error) {
      console.error(`Failed to get proposal ${proposalId}:`, error.message);
      throw error;
    }
  }

  // Get carbon credits
  async getCarbonCredits(options = {}) {
    try {
      const params = new URLSearchParams({
        projectType: options.projectType,
        vintage: options.vintage,
        certification: options.certification,
        page: options.page || 1,
        limit: options.limit || 50,
      });

      const response = await this.client.get(`/v1/carbon-credits?${params}`);
      return response.data;
    } catch (error) {
      console.error('Failed to get carbon credits:', error.message);
      throw error;
    }
  }
}
```

---

## 3. Real-Time Integration

### WebSocket Connection

```javascript
// lib/websocket-client.js
const WebSocket = require('ws');
const EventEmitter = require('events');

class WRFCoinWebSocketClient extends EventEmitter {
  constructor(config = {}) {
    super();

    this.config = {
      wsUrl: config.wsUrl || 'wss://ws.wrfcoin.network',
      apiKey: config.apiKey || process.env.WRFCOIN_API_KEY,
      reconnectInterval: config.reconnectInterval || 5000,
      maxReconnectAttempts: config.maxReconnectAttempts || 10,
      pingInterval: config.pingInterval || 30000,
      ...config,
    };

    this.ws = null;
    this.reconnectAttempts = 0;
    this.subscriptions = new Set();
    this.isConnected = false;
    this.pingTimer = null;
  }

  connect() {
    console.log('🔌 Connecting to WRFCoin WebSocket...');

    this.ws = new WebSocket(this.config.wsUrl, {
      headers: {
        Authorization: `Bearer ${this.config.apiKey}`,
        'User-Agent': 'WRFCoin-WS-Client/1.0.0',
      },
    });

    this.ws.on('open', () => {
      console.log('✅ WebSocket connected');
      this.isConnected = true;
      this.reconnectAttempts = 0;
      this.emit('connected');

      // Start ping/pong
      this._startPing();

      // Resubscribe to channels
      this._resubscribe();
    });

    this.ws.on('message', data => {
      try {
        const message = JSON.parse(data.toString());
        this._handleMessage(message);
      } catch (error) {
        console.error('❌ Failed to parse WebSocket message:', error);
      }
    });

    this.ws.on('close', (code, reason) => {
      console.log(`🔌 WebSocket disconnected: ${code} - ${reason}`);
      this.isConnected = false;
      this._stopPing();
      this.emit('disconnected', { code, reason });
      this._reconnect();
    });

    this.ws.on('error', error => {
      console.error('❌ WebSocket error:', error);
      this.emit('error', error);
    });

    this.ws.on('pong', () => {
      console.log('🏓 Pong received');
    });
  }

  disconnect() {
    console.log('🔌 Disconnecting WebSocket...');
    this.reconnectAttempts = this.config.maxReconnectAttempts; // Prevent reconnection
    this._stopPing();

    if (this.ws) {
      this.ws.close(1000, 'Client disconnect');
    }
  }

  subscribe(channel, params = {}) {
    const subscription = {
      action: 'subscribe',
      channel: channel,
      params: params,
    };

    this.subscriptions.add(JSON.stringify(subscription));

    if (this.isConnected) {
      this.ws.send(JSON.stringify(subscription));
      console.log(`📡 Subscribed to ${channel}`);
    }
  }

  unsubscribe(channel, params = {}) {
    const subscription = {
      action: 'unsubscribe',
      channel: channel,
      params: params,
    };

    this.subscriptions.delete(
      JSON.stringify({
        action: 'subscribe',
        channel: channel,
        params: params,
      })
    );

    if (this.isConnected) {
      this.ws.send(JSON.stringify(subscription));
      console.log(`📡 Unsubscribed from ${channel}`);
    }
  }

  _handleMessage(message) {
    switch (message.type) {
      case 'subscription_success':
        console.log(`✅ Subscription confirmed: ${message.channel}`);
        this.emit('subscribed', message.channel);
        break;

      case 'subscription_error':
        console.error(`❌ Subscription error: ${message.error}`);
        this.emit('subscription_error', message);
        break;

      case 'data':
        this.emit('data', message.channel, message.data);
        this.emit(`data:${message.channel}`, message.data);
        break;

      case 'error':
        console.error(`❌ Server error: ${message.error}`);
        this.emit('server_error', message);
        break;

      default:
        console.log('📨 Unknown message type:', message.type);
        this.emit('unknown_message', message);
    }
  }

  _startPing() {
    this.pingTimer = setInterval(() => {
      if (this.ws && this.ws.readyState === WebSocket.OPEN) {
        this.ws.ping();
        console.log('🏓 Ping sent');
      }
    }, this.config.pingInterval);
  }

  _stopPing() {
    if (this.pingTimer) {
      clearInterval(this.pingTimer);
      this.pingTimer = null;
    }
  }

  _reconnect() {
    if (this.reconnectAttempts >= this.config.maxReconnectAttempts) {
      console.error('❌ Max reconnection attempts reached');
      this.emit('max_reconnect_attempts');
      return;
    }

    this.reconnectAttempts++;
    const delay = this.config.reconnectInterval * this.reconnectAttempts;

    console.log(`🔄 Reconnecting in ${delay}ms (attempt ${this.reconnectAttempts})`);

    setTimeout(() => {
      if (!this.isConnected) {
        this.connect();
      }
    }, delay);
  }

  _resubscribe() {
    this.subscriptions.forEach(subscriptionStr => {
      const subscription = JSON.parse(subscriptionStr);
      this.ws.send(JSON.stringify(subscription));
      console.log(`📡 Resubscribed to ${subscription.channel}`);
    });
  }
}

module.exports = WRFCoinWebSocketClient;
```

### Webhook Integration

```javascript
// lib/webhook-server.js
const express = require('express');
const crypto = require('crypto');
const bodyParser = require('body-parser');

class WebhookServer {
  constructor(config = {}) {
    this.config = {
      port: config.port || 3000,
      webhookSecret: config.webhookSecret || process.env.WRFCOIN_WEBHOOK_SECRET,
      path: config.path || '/webhooks/wrfcoin',
      ...config,
    };

    this.app = express();
    this.handlers = new Map();
    this._setupMiddleware();
    this._setupRoutes();
  }

  _setupMiddleware() {
    // Raw body parser for signature verification
    this.app.use(this.config.path, bodyParser.raw({ type: 'application/json' }));

    // JSON parser for other routes
    this.app.use(bodyParser.json());

    // Logging middleware
    this.app.use((req, res, next) => {
      console.log(`📨 ${req.method} ${req.path} - ${new Date().toISOString()}`);
      next();
    });
  }

  _setupRoutes() {
    // Health check endpoint
    this.app.get('/health', (req, res) => {
      res.json({ status: 'healthy', timestamp: new Date().toISOString() });
    });

    // Main webhook endpoint
    this.app.post(this.config.path, (req, res) => {
      this._handleWebhook(req, res);
    });
  }

  async _handleWebhook(req, res) {
    try {
      // Verify signature
      if (!this._verifySignature(req)) {
        console.error('❌ Invalid webhook signature');
        return res.status(401).json({ error: 'Invalid signature' });
      }

      // Parse payload
      const payload = JSON.parse(req.body.toString());
      const eventType = req.headers['x-wrfcoin-event-type'];

      console.log(`📨 Webhook received: ${eventType}`);

      // Handle event
      const handler = this.handlers.get(eventType);
      if (handler) {
        await handler(payload, req.headers);
        console.log(`✅ Webhook handled: ${eventType}`);
      } else {
        console.warn(`⚠️  No handler for event type: ${eventType}`);
      }

      res.status(200).json({ status: 'received' });
    } catch (error) {
      console.error('❌ Webhook processing error:', error);
      res.status(500).json({ error: 'Internal server error' });
    }
  }

  _verifySignature(req) {
    const signature = req.headers['x-wrfcoin-signature'];
    if (!signature) return false;

    const expectedSignature = crypto
      .createHmac('sha256', this.config.webhookSecret)
      .update(req.body)
      .digest('hex');

    return signature === `sha256=${expectedSignature}`;
  }

  // Register event handlers
  on(eventType, handler) {
    this.handlers.set(eventType, handler);
    console.log(`📝 Registered handler for: ${eventType}`);
  }

  // Remove event handlers
  off(eventType) {
    this.handlers.delete(eventType);
    console.log(`🗑️  Removed handler for: ${eventType}`);
  }

  // Start server
  listen() {
    return new Promise((resolve, reject) => {
      try {
        const server = this.app.listen(this.config.port, () => {
          console.log(`🚀 Webhook server listening on port ${this.config.port}`);
          console.log(`📡 Webhook endpoint: ${this.config.path}`);
          resolve(server);
        });
      } catch (error) {
        reject(error);
      }
    });
  }
}

module.exports = WebhookServer;
```

---

## 4. Complete Integration Example

### Full-Featured Application

```javascript
// examples/complete-api-integration.js
const WRFCoinAPIClient = require('../lib/api-client');
const BlockchainAPI = require('../lib/blockchain-api');
const ClimateAPI = require('../lib/climate-api');
const DeFiAPI = require('../lib/defi-api');
const WRFCoinWebSocketClient = require('../lib/websocket-client');
const WebhookServer = require('../lib/webhook-server');

class WRFCoinApp {
  constructor(config = {}) {
    this.config = config;

    // Initialize API clients
    this.blockchainAPI = new BlockchainAPI(config);
    this.climateAPI = new ClimateAPI(config);
    this.defiAPI = new DeFiAPI(config);

    // Initialize WebSocket client
    this.wsClient = new WRFCoinWebSocketClient(config);

    // Initialize webhook server
    this.webhookServer = new WebhookServer(config);

    this.isRunning = false;
    this._setupEventHandlers();
  }

  _setupEventHandlers() {
    // WebSocket event handlers
    this.wsClient.on('connected', () => {
      console.log('🌐 Real-time connection established');
      this._subscribeToChannels();
    });

    this.wsClient.on('data:blocks', data => {
      console.log('🧱 New block:', data.blockNumber);
      this._handleNewBlock(data);
    });

    this.wsClient.on('data:climate', data => {
      console.log('🌡️  New climate data:', data.stationId);
      this._handleNewClimateData(data);
    });

    this.wsClient.on('data:transactions', data => {
      console.log('💸 New transaction:', data.hash);
      this._handleNewTransaction(data);
    });

    // Webhook event handlers
    this.webhookServer.on('data.validated', payload => {
      console.log('✅ Data validation completed:', payload.dataId);
      this._handleDataValidation(payload);
    });

    this.webhookServer.on('governance.proposal.created', payload => {
      console.log('🗳️  New governance proposal:', payload.proposalId);
      this._handleNewProposal(payload);
    });

    this.webhookServer.on('staking.reward.distributed', payload => {
      console.log('💰 Staking reward distributed:', payload.amount);
      this._handleStakingReward(payload);
    });
  }

  async start() {
    console.log('🚀 Starting WRFCoin application...');

    try {
      // Start webhook server
      await this.webhookServer.listen();

      // Connect to WebSocket
      this.wsClient.connect();

      // Initialize data monitoring
      await this._initializeMonitoring();

      this.isRunning = true;
      console.log('✅ WRFCoin application started successfully');
    } catch (error) {
      console.error('❌ Failed to start application:', error);
      throw error;
    }
  }

  async stop() {
    console.log('🛑 Stopping WRFCoin application...');

    this.isRunning = false;
    this.wsClient.disconnect();

    console.log('✅ Application stopped');
  }

  async _initializeMonitoring() {
    // Get initial network state
    const networkInfo = await this.blockchainAPI.getNetworkInfo();
    console.log('📊 Network info:', {
      name: networkInfo.name,
      blockHeight: networkInfo.blockHeight,
      totalTransactions: networkInfo.totalTransactions,
    });

    // Get recent climate data
    const recentClimateData = await this.climateAPI.getClimateData({
      startDate: new Date(Date.now() - 24 * 60 * 60 * 1000).toISOString(), // Last 24 hours
      limit: 10,
    });
    console.log(`🌡️  Recent climate data: ${recentClimateData.data.length} entries`);

    // Get active governance proposals
    const proposals = await this.defiAPI.getGovernanceProposals({
      status: 'active',
      limit: 5,
    });
    console.log(`🗳️  Active governance proposals: ${proposals.data.length}`);
  }

  _subscribeToChannels() {
    // Subscribe to new blocks
    this.wsClient.subscribe('blocks', {
      includeTransactions: false,
    });

    // Subscribe to climate data updates
    this.wsClient.subscribe('climate', {
      regions: ['global'],
      dataTypes: ['temperature', 'humidity', 'pressure'],
    });

    // Subscribe to large transactions
    this.wsClient.subscribe('transactions', {
      minValue: 1000, // Transactions > 1000 WRF
      addresses: [], // Can specify specific addresses to monitor
    });
  }

  async _handleNewBlock(blockData) {
    // Process new block
    console.log(
      `📈 Block ${blockData.blockNumber} processed with ${blockData.transactionCount} transactions`
    );

    // Update local cache or database
    // Trigger any block-based calculations
  }

  async _handleNewClimateData(climateData) {
    // Process new climate data
    console.log(
      `🌍 Climate data from station ${climateData.stationId}: ${climateData.temperature}°C`
    );

    // Validate data quality
    if (this._isAnomalousData(climateData)) {
      console.warn('⚠️  Anomalous climate data detected, flagging for review');
      await this._flagAnomalousData(climateData);
    }
  }

  async _handleNewTransaction(txData) {
    // Process large transactions
    console.log(`💰 Large transaction: ${txData.value} WRF from ${txData.from} to ${txData.to}`);

    // Update analytics
    // Send notifications if needed
  }

  async _handleDataValidation(payload) {
    // Handle validated climate data
    if (payload.approved) {
      console.log(`✅ Data ${payload.dataId} approved and rewarded`);

      // Update local records
      // Trigger any post-validation processes
    } else {
      console.log(`❌ Data ${payload.dataId} rejected: ${payload.reason}`);

      // Handle rejection
      // Notify data submitter
    }
  }

  async _handleNewProposal(payload) {
    // Handle new governance proposals
    console.log(`🗳️  New proposal: ${payload.title}`);

    // Analyze proposal
    // Send notifications to stakeholders
    // Update governance dashboard
  }

  async _handleStakingReward(payload) {
    // Handle staking rewards
    console.log(`💰 Staking reward: ${payload.amount} WRF to ${payload.recipient}`);

    // Update reward tracking
    // Calculate yield rates
  }

  _isAnomalousData(data) {
    // Simple anomaly detection
    return (
      data.temperature < -50 ||
      data.temperature > 60 ||
      data.humidity < 0 ||
      data.humidity > 100 ||
      data.pressure < 800 ||
      data.pressure > 1200
    );
  }

  async _flagAnomalousData(data) {
    // Flag anomalous data for manual review
    console.log(`🚩 Flagging anomalous data: ${data.id}`);
    // Implementation would depend on your review system
  }

  // Public methods for external use
  async getNetworkStats() {
    const [networkInfo, stakingInfo] = await Promise.all([
      this.blockchainAPI.getNetworkInfo(),
      this.defiAPI.getStakingInfo(),
    ]);

    return {
      network: networkInfo,
      staking: stakingInfo,
      timestamp: new Date().toISOString(),
    };
  }

  async getClimateStats(region = 'global') {
    const endDate = new Date();
    const startDate = new Date(endDate.getTime() - 7 * 24 * 60 * 60 * 1000); // Last 7 days

    const climateData = await this.climateAPI.getClimateData({
      startDate: startDate.toISOString(),
      endDate: endDate.toISOString(),
      aggregation: 'daily',
    });

    return {
      region,
      data: climateData.data,
      period: { start: startDate, end: endDate },
      totalEntries: climateData.total,
    };
  }
}

// Usage example
async function main() {
  const app = new WRFCoinApp({
    apiKey: process.env.WRFCOIN_API_KEY,
    apiSecret: process.env.WRFCOIN_API_SECRET,
    network: 'testnet',
    wsUrl: 'wss://ws.testnet.wrfcoin.network',
    webhookSecret: process.env.WRFCOIN_WEBHOOK_SECRET,
    port: 3000,
  });

  try {
    await app.start();

    // Keep the application running
    process.on('SIGINT', async () => {
      console.log('\n🛑 Received SIGINT, shutting down gracefully...');
      await app.stop();
      process.exit(0);
    });

    // Example of periodic tasks
    setInterval(async () => {
      const stats = await app.getNetworkStats();
      console.log('📊 Network stats:', stats.network.blockHeight, 'blocks');
    }, 60000); // Every minute
  } catch (error) {
    console.error('❌ Application failed:', error);
    process.exit(1);
  }
}

if (require.main === module) {
  main().catch(console.error);
}

module.exports = WRFCoinApp;
```

---

## 5. Performance Optimization

### Caching and Rate Limiting

```javascript
// lib/performance-optimizations.js
const NodeCache = require('node-cache');
const { RateLimiterMemory } = require('rate-limiter-flexible');

class PerformanceOptimizedAPIClient extends WRFCoinAPIClient {
  constructor(config) {
    super(config);

    // Initialize cache
    this.cache = new NodeCache({
      stdTTL: config.cacheTTL || 300, // 5 minutes default
      checkperiod: 60, // Check for expired keys every minute
    });

    // Initialize rate limiter
    this.rateLimiter = new RateLimiterMemory({
      keyPrefix: 'wrfcoin_api',
      points: config.rateLimit || 100, // Number of requests
      duration: 60, // Per 60 seconds
    });

    this.cacheHits = 0;
    this.cacheMisses = 0;
  }

  async request(method, endpoint, data = null, options = {}) {
    // Check rate limiting
    try {
      await this.rateLimiter.consume('global');
    } catch (rateLimiterRes) {
      const msBeforeNext = rateLimiterRes.msBeforeNext;
      throw new Error(`Rate limit exceeded. Try again in ${msBeforeNext}ms`);
    }

    // Check cache for GET requests
    if (method === 'GET' && !options.skipCache) {
      const cacheKey = this._getCacheKey(endpoint, data);
      const cachedResult = this.cache.get(cacheKey);

      if (cachedResult) {
        this.cacheHits++;
        console.log(`💾 Cache hit for ${endpoint}`);
        return cachedResult;
      }
      this.cacheMisses++;
    }

    // Make API request
    let result;
    if (method === 'GET') {
      result = await this.client.get(endpoint, { params: data });
    } else if (method === 'POST') {
      result = await this.client.post(endpoint, data);
    } else if (method === 'PUT') {
      result = await this.client.put(endpoint, data);
    } else if (method === 'DELETE') {
      result = await this.client.delete(endpoint);
    }

    // Cache GET responses
    if (method === 'GET' && !options.skipCache) {
      const cacheKey = this._getCacheKey(endpoint, data);
      const ttl = options.cacheTTL || this.cache.options.stdTTL;
      this.cache.set(cacheKey, result, ttl);
    }

    return result;
  }

  _getCacheKey(endpoint, params = {}) {
    const paramString = Object.keys(params)
      .sort()
      .map(key => `${key}=${params[key]}`)
      .join('&');
    return `${endpoint}?${paramString}`;
  }

  // Cache statistics
  getCacheStats() {
    return {
      hits: this.cacheHits,
      misses: this.cacheMisses,
      hitRate: this.cacheHits / (this.cacheHits + this.cacheMisses),
      keys: this.cache.keys().length,
    };
  }

  // Clear cache
  clearCache(pattern = null) {
    if (pattern) {
      const keys = this.cache.keys().filter(key => key.includes(pattern));
      this.cache.del(keys);
      console.log(`🗑️  Cleared ${keys.length} cache entries matching "${pattern}"`);
    } else {
      this.cache.flushAll();
      console.log('🗑️  Cache cleared');
    }
  }
}
```

---

## Next Steps

1. **Explore [GraphQL API Integration](./02-graphql-integration.md)**
2. **Build [End-to-End Applications](../applications/)**
3. **Implement [Real-time Dashboards](../applications/02-realtime-dashboard.md)**

---

## API Reference

### Base URLs

- **Mainnet**: `https://api.wrfcoin.network`
- **Testnet**: `https://api.testnet.wrfcoin.network`

### Authentication

- **Method**: HMAC-SHA256 signature
- **Headers**: `X-API-Key`, `X-Timestamp`, `X-Signature`

### Rate Limits

- **Default**: 100 requests per minute
- **Burst**: Up to 1000 requests per hour
- **Headers**: `X-RateLimit-Limit`, `X-RateLimit-Remaining`

This comprehensive REST API integration guide provides the foundation for building robust
applications with the WRFCoin platform.
