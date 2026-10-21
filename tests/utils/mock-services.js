/**
 * Mock Services for Testing
 * Provides mock implementations of external services
 */

const express = require('express');
const { WebSocketServer } = require('ws');
const { MOCK_SERVICES, TEST_DATA } = require('../test.config');

class MockWeatherService {
  constructor(port = MOCK_SERVICES.weather.port) {
    this.app = express();
    this.server = null;
    this.port = port;
    this.setupRoutes();
  }

  setupRoutes() {
    this.app.use(express.json());

    // Current weather endpoint
    this.app.get('/api/weather/current', (req, res) => {
      const { lat, lon } = req.query;

      setTimeout(() => {
        res.json({
          location: {
            latitude: parseFloat(lat) || 40.7128,
            longitude: parseFloat(lon) || -74.006,
          },
          temperature: 22.5 + Math.random() * 10,
          humidity: 60 + Math.random() * 20,
          pressure: 1013.25 + Math.random() * 10,
          windSpeed: 5 + Math.random() * 15,
          windDirection: Math.floor(Math.random() * 360),
          timestamp: new Date().toISOString(),
        });
      }, 100);
    });

    // Forecast endpoint
    this.app.get('/api/weather/forecast', (req, res) => {
      const forecast = Array.from({ length: 7 }, (_, i) => ({
        date: new Date(Date.now() + i * 24 * 60 * 60 * 1000).toISOString(),
        tempMin: 15 + Math.random() * 5,
        tempMax: 25 + Math.random() * 5,
        precipitation: Math.random() * 0.3,
        humidity: 60 + Math.random() * 20,
      }));

      setTimeout(() => {
        res.json({ forecast });
      }, 200);
    });

    // Historical data endpoint
    this.app.get('/api/weather/historical', (req, res) => {
      const { startDate, endDate } = req.query;
      const data = Array.from({ length: 30 }, (_, i) => ({
        date: new Date(Date.now() - i * 24 * 60 * 60 * 1000).toISOString(),
        temperature: 20 + Math.random() * 10,
        humidity: 60 + Math.random() * 20,
        pressure: 1013.25 + Math.random() * 10,
      }));

      setTimeout(() => {
        res.json({ data });
      }, 150);
    });

    // IoT device data endpoint
    this.app.post('/api/weather/iot', (req, res) => {
      const { deviceId, data } = req.body;
      res.json({
        success: true,
        deviceId,
        timestamp: new Date().toISOString(),
        processed: true,
      });
    });
  }

  async start() {
    return new Promise(resolve => {
      this.server = this.app.listen(this.port, () => {
        console.log(`✅ Mock Weather Service started on port ${this.port}`);
        resolve();
      });
    });
  }

  async stop() {
    if (this.server) {
      return new Promise(resolve => {
        this.server.close(() => {
          console.log('✅ Mock Weather Service stopped');
          resolve();
        });
      });
    }
  }
}

class MockBlockchainService {
  constructor(port = MOCK_SERVICES.blockchain.port) {
    this.app = express();
    this.server = null;
    this.port = port;
    this.chainId = MOCK_SERVICES.blockchain.chainId;
    this.blockNumber = 1000;
    this.accounts = new Map();
    this.setupRoutes();
    this.initializeAccounts();
  }

  initializeAccounts() {
    // Initialize test accounts with balances
    this.accounts.set(TEST_DATA.wallets.primary.address, {
      balance: '1000000000000000000000', // 1000 WRF
      nonce: 0,
    });
    this.accounts.set(TEST_DATA.wallets.secondary.address, {
      balance: '500000000000000000000', // 500 WRF
      nonce: 0,
    });
  }

  setupRoutes() {
    this.app.use(express.json());

    // JSON-RPC endpoint
    this.app.post('/', (req, res) => {
      const { method, params, id } = req.body;

      switch (method) {
        case 'eth_chainId':
          res.json({
            jsonrpc: '2.0',
            id,
            result: `0x${this.chainId.toString(16)}`,
          });
          break;

        case 'eth_blockNumber':
          res.json({
            jsonrpc: '2.0',
            id,
            result: `0x${this.blockNumber.toString(16)}`,
          });
          break;

        case 'eth_getBalance':
          const [address] = params;
          const account = this.accounts.get(address.toLowerCase()) || {
            balance: '0',
          };
          res.json({
            jsonrpc: '2.0',
            id,
            result: `0x${BigInt(account.balance).toString(16)}`,
          });
          break;

        case 'eth_sendTransaction':
          const tx = params[0];
          const txHash = `0x${Date.now().toString(16)}${Math.random().toString(16).substr(2, 8)}`;

          // Update balances
          if (tx.from && tx.to && tx.value) {
            const fromAccount = this.accounts.get(tx.from.toLowerCase());
            const toAccount = this.accounts.get(tx.to.toLowerCase()) || {
              balance: '0',
              nonce: 0,
            };

            if (fromAccount) {
              const value = BigInt(tx.value);
              fromAccount.balance = (
                BigInt(fromAccount.balance) - value
              ).toString();
              toAccount.balance = (
                BigInt(toAccount.balance) + value
              ).toString();
              fromAccount.nonce++;

              this.accounts.set(tx.to.toLowerCase(), toAccount);
            }
          }

          res.json({ jsonrpc: '2.0', id, result: txHash });
          break;

        case 'eth_getTransactionReceipt':
          const [hash] = params;
          res.json({
            jsonrpc: '2.0',
            id,
            result: {
              transactionHash: hash,
              blockNumber: `0x${this.blockNumber.toString(16)}`,
              status: '0x1',
              gasUsed: '0x5208',
              cumulativeGasUsed: '0x5208',
              logs: [],
            },
          });
          break;

        default:
          res.json({
            jsonrpc: '2.0',
            id,
            error: { code: -32601, message: 'Method not found' },
          });
      }
    });

    // Block generation simulation
    setInterval(() => {
      this.blockNumber++;
    }, 5000);
  }

  async start() {
    return new Promise(resolve => {
      this.server = this.app.listen(this.port, () => {
        console.log(`✅ Mock Blockchain Service started on port ${this.port}`);
        resolve();
      });
    });
  }

  async stop() {
    if (this.server) {
      return new Promise(resolve => {
        this.server.close(() => {
          console.log('✅ Mock Blockchain Service stopped');
          resolve();
        });
      });
    }
  }
}

class MockWebSocketService {
  constructor(port = 5001) {
    this.wss = null;
    this.port = port;
    this.clients = new Set();
    this.intervals = [];
  }

  async start() {
    this.wss = new WebSocketServer({ port: this.port });

    this.wss.on('connection', ws => {
      console.log('✅ WebSocket client connected');
      this.clients.add(ws);

      // Send initial connection message
      ws.send(
        JSON.stringify({
          type: 'connection',
          status: 'connected',
          timestamp: new Date().toISOString(),
        })
      );

      // Handle messages
      ws.on('message', data => {
        try {
          const message = JSON.parse(data.toString());
          this.handleMessage(ws, message);
        } catch (error) {
          ws.send(
            JSON.stringify({
              type: 'error',
              message: 'Invalid message format',
            })
          );
        }
      });

      // Handle disconnection
      ws.on('close', () => {
        console.log('WebSocket client disconnected');
        this.clients.delete(ws);
      });
    });

    // Start mock data streams
    this.startDataStreams();

    console.log(`✅ Mock WebSocket Service started on port ${this.port}`);
  }

  handleMessage(ws, message) {
    switch (message.type) {
      case 'subscribe':
        ws.send(
          JSON.stringify({
            type: 'subscribed',
            channel: message.channel,
            timestamp: new Date().toISOString(),
          })
        );
        break;

      case 'unsubscribe':
        ws.send(
          JSON.stringify({
            type: 'unsubscribed',
            channel: message.channel,
            timestamp: new Date().toISOString(),
          })
        );
        break;

      default:
        ws.send(
          JSON.stringify({
            type: 'echo',
            data: message,
            timestamp: new Date().toISOString(),
          })
        );
    }
  }

  startDataStreams() {
    // Price updates stream
    this.intervals.push(
      setInterval(() => {
        const priceUpdate = {
          type: 'price_update',
          data: {
            symbol: 'WRF/USD',
            price: 10 + Math.random() * 2,
            volume: Math.floor(Math.random() * 1000000),
            change24h: -5 + Math.random() * 10,
            timestamp: new Date().toISOString(),
          },
        };

        this.broadcast(priceUpdate);
      }, 5000)
    );

    // Block updates stream
    this.intervals.push(
      setInterval(() => {
        const blockUpdate = {
          type: 'new_block',
          data: {
            number: Math.floor(Date.now() / 5000),
            hash: `0x${Math.random().toString(16).substr(2, 64)}`,
            transactions: Math.floor(Math.random() * 100),
            timestamp: new Date().toISOString(),
          },
        };

        this.broadcast(blockUpdate);
      }, 5000)
    );

    // Network stats stream
    this.intervals.push(
      setInterval(() => {
        const networkStats = {
          type: 'network_stats',
          data: {
            nodes: 100 + Math.floor(Math.random() * 50),
            tps: 80 + Math.floor(Math.random() * 40),
            difficulty: Math.floor(Math.random() * 1000000),
            hashrate: Math.floor(Math.random() * 100000),
            timestamp: new Date().toISOString(),
          },
        };

        this.broadcast(networkStats);
      }, 10000)
    );
  }

  broadcast(message) {
    const data = JSON.stringify(message);
    this.clients.forEach(client => {
      if (client.readyState === 1) {
        // WebSocket.OPEN
        client.send(data);
      }
    });
  }

  async stop() {
    // Clear intervals
    this.intervals.forEach(interval => clearInterval(interval));
    this.intervals = [];

    // Close all connections
    this.clients.forEach(client => client.close());
    this.clients.clear();

    // Close server
    if (this.wss) {
      await new Promise(resolve => {
        this.wss.close(() => {
          console.log('✅ Mock WebSocket Service stopped');
          resolve();
        });
      });
    }
  }
}

class MockExternalServices {
  constructor(port = MOCK_SERVICES.external.port) {
    this.app = express();
    this.server = null;
    this.port = port;
    this.setupRoutes();
  }

  setupRoutes() {
    this.app.use(express.json());

    // NOAA Weather Service Mock
    this.app.get('/api/noaa/*', (req, res) => {
      res.json({
        service: 'NOAA',
        status: 'operational',
        data: {
          temperature: 22.5,
          conditions: 'partly cloudy',
          forecast: 'mild temperatures expected',
        },
      });
    });

    // IoTeX Device Registry Mock
    this.app.post('/api/iotex/devices', (req, res) => {
      res.json({
        success: true,
        deviceId: `iotex_${Date.now()}`,
        status: 'registered',
      });
    });

    // Climate Data Service Mock
    this.app.get('/api/climate/*', (req, res) => {
      res.json({
        service: 'Climate Data',
        data: {
          co2Levels: 415.2,
          temperatureAnomaly: 1.2,
          seaLevelRise: 3.4,
        },
      });
    });
  }

  async start() {
    return new Promise(resolve => {
      this.server = this.app.listen(this.port, () => {
        console.log(`✅ Mock External Services started on port ${this.port}`);
        resolve();
      });
    });
  }

  async stop() {
    if (this.server) {
      return new Promise(resolve => {
        this.server.close(() => {
          console.log('✅ Mock External Services stopped');
          resolve();
        });
      });
    }
  }
}

// Service Manager
class MockServiceManager {
  constructor() {
    this.services = {};
  }

  async startAll() {
    console.log('🚀 Starting all mock services...\n');

    if (MOCK_SERVICES.weather.enabled) {
      this.services.weather = new MockWeatherService();
      await this.services.weather.start();
    }

    if (MOCK_SERVICES.blockchain.enabled) {
      this.services.blockchain = new MockBlockchainService();
      await this.services.blockchain.start();
    }

    if (MOCK_SERVICES.external.enabled) {
      this.services.external = new MockExternalServices();
      await this.services.external.start();
    }

    // WebSocket service (always needed for real-time tests)
    this.services.websocket = new MockWebSocketService();
    await this.services.websocket.start();

    console.log('\n✅ All mock services started successfully');
  }

  async stopAll() {
    console.log('\n🛑 Stopping all mock services...');

    for (const [name, service] of Object.entries(this.services)) {
      await service.stop();
    }

    this.services = {};
    console.log('✅ All mock services stopped');
  }
}

// Singleton instance
let manager = null;

module.exports = {
  MockWeatherService,
  MockBlockchainService,
  MockWebSocketService,
  MockExternalServices,

  getMockServiceManager() {
    if (!manager) {
      manager = new MockServiceManager();
    }
    return manager;
  },
};
