// Mock Services for WRFCoin Test Data Management
import { Page, Route } from '@playwright/test';

export interface MockAPIResponse {
  endpoint: string;
  method: 'GET' | 'POST' | 'PUT' | 'DELETE';
  response: any;
  status?: number;
  delay?: number;
  conditional?: (request: any) => boolean;
}

export interface MockServiceConfig {
  weather: MockAPIResponse[];
  blockchain: MockAPIResponse[];
  defi: MockAPIResponse[];
  governance: MockAPIResponse[];
  bridges: MockAPIResponse[];
  iot: MockAPIResponse[];
}

export class MockServiceManager {
  private activeMocks: Map<string, Route> = new Map();
  private page: Page;

  constructor(page: Page) {
    this.page = page;
  }

  // Setup comprehensive mock services
  async setupMockServices(
    config: Partial<MockServiceConfig> = {}
  ): Promise<void> {
    console.log('Setting up mock services for testing...');

    // Setup weather API mocks
    if (config.weather) {
      await this.setupWeatherMocks(config.weather);
    } else {
      await this.setupDefaultWeatherMocks();
    }

    // Setup blockchain API mocks
    if (config.blockchain) {
      await this.setupBlockchainMocks(config.blockchain);
    } else {
      await this.setupDefaultBlockchainMocks();
    }

    // Setup DeFi protocol mocks
    if (config.defi) {
      await this.setupDeFiMocks(config.defi);
    } else {
      await this.setupDefaultDeFiMocks();
    }

    // Setup governance mocks
    if (config.governance) {
      await this.setupGovernanceMocks(config.governance);
    } else {
      await this.setupDefaultGovernanceMocks();
    }

    // Setup cross-chain bridge mocks
    if (config.bridges) {
      await this.setupBridgeMocks(config.bridges);
    } else {
      await this.setupDefaultBridgeMocks();
    }

    // Setup IoT sensor mocks
    if (config.iot) {
      await this.setupIoTMocks(config.iot);
    } else {
      await this.setupDefaultIoTMocks();
    }

    console.log('Mock services setup completed');
  }

  // Weather service mocks
  private async setupDefaultWeatherMocks(): Promise<void> {
    const weatherMocks: MockAPIResponse[] = [
      {
        endpoint: '**/api/weather/current**',
        method: 'GET',
        response: {
          temperature: 22.5,
          humidity: 65,
          pressure: 1013.25,
          windSpeed: 12.3,
          windDirection: 180,
          timestamp: new Date().toISOString(),
          qualityScore: 0.95,
        },
      },
      {
        endpoint: '**/api/weather/historical**',
        method: 'GET',
        response: {
          data: Array.from({ length: 100 }, (_, i) => ({
            timestamp: new Date(Date.now() - i * 300000).toISOString(),
            temperature: 20 + Math.random() * 10,
            humidity: 50 + Math.random() * 30,
            pressure: 1000 + Math.random() * 50,
            windSpeed: Math.random() * 20,
            windDirection: Math.random() * 360,
          })),
        },
      },
      {
        endpoint: '**/api/weather/submit',
        method: 'POST',
        response: {
          success: true,
          dataId: 'weather_' + Math.random().toString(36).substr(2, 9),
          qualityScore: 0.85 + Math.random() * 0.15,
          rewards: {
            amount: (1 + Math.random() * 5).toFixed(2),
            currency: 'WRF',
          },
        },
      },
      {
        endpoint: '**/api/weather/validate',
        method: 'POST',
        response: {
          valid: true,
          qualityMetrics: {
            accuracy: 0.92,
            completeness: 0.98,
            consistency: 0.89,
            timeliness: 0.95,
          },
          issues: [],
        },
      },
    ];

    await this.setupMockResponses(weatherMocks);
  }

  // Blockchain service mocks
  private async setupDefaultBlockchainMocks(): Promise<void> {
    const blockchainMocks: MockAPIResponse[] = [
      {
        endpoint: '**/api/blockchain/balance**',
        method: 'GET',
        response: {
          WRF: '1250.75',
          ETH: '2.34',
          CARBON: '890.12',
        },
      },
      {
        endpoint: '**/api/blockchain/transaction',
        method: 'POST',
        response: {
          hash: '0x' + Math.random().toString(16).substr(2, 64),
          status: 'pending',
          gasUsed: '21000',
          blockNumber: Math.floor(Math.random() * 1000000),
          confirmations: 0,
        },
        delay: 2000,
      },
      {
        endpoint: '**/api/blockchain/nft/mint',
        method: 'POST',
        response: {
          tokenId: Math.floor(Math.random() * 10000).toString(),
          contractAddress: '0x' + Math.random().toString(16).substr(2, 40),
          transactionHash: '0x' + Math.random().toString(16).substr(2, 64),
          metadata: {
            name: 'Weather Computational NFT',
            description: 'NFT representing weather data computation',
            image: 'https://picsum.photos/400/400',
          },
        },
      },
      {
        endpoint: '**/api/blockchain/gas-price',
        method: 'GET',
        response: {
          slow: '20',
          standard: '35',
          fast: '50',
          instant: '80',
        },
      },
    ];

    await this.setupMockResponses(blockchainMocks);
  }

  // DeFi service mocks
  private async setupDefaultDeFiMocks(): Promise<void> {
    const defiMocks: MockAPIResponse[] = [
      {
        endpoint: '**/api/defi/pools',
        method: 'GET',
        response: [
          {
            id: 'WRF-ETH',
            token0: 'WRF',
            token1: 'ETH',
            reserve0: '1000000',
            reserve1: '500',
            apy: 0.15,
            tvl: '2500000',
            fees24h: '1250',
          },
          {
            id: 'WRF-CARBON',
            token0: 'WRF',
            token1: 'CARBON',
            reserve0: '800000',
            reserve1: '400000',
            apy: 0.22,
            tvl: '1800000',
            fees24h: '980',
          },
        ],
      },
      {
        endpoint: '**/api/defi/add-liquidity',
        method: 'POST',
        response: {
          success: true,
          lpTokens: '156.78',
          transactionHash: '0x' + Math.random().toString(16).substr(2, 64),
          position: {
            poolId: 'WRF-ETH',
            amount0: '1000',
            amount1: '0.5',
            share: '0.012',
          },
        },
      },
      {
        endpoint: '**/api/defi/stake',
        method: 'POST',
        response: {
          success: true,
          stakingId: 'stake_' + Math.random().toString(36).substr(2, 9),
          stakedAmount: '500',
          apy: 0.18,
          maturityDate: new Date(
            Date.now() + 30 * 24 * 60 * 60 * 1000
          ).toISOString(),
          rewards: {
            token: 'WRF',
            dailyRate: '0.25',
          },
        },
      },
      {
        endpoint: '**/api/defi/harvest',
        method: 'POST',
        response: {
          success: true,
          rewards: [
            { token: 'WRF', amount: '12.45' },
            { token: 'CARBON', amount: '5.67' },
          ],
          transactionHash: '0x' + Math.random().toString(16).substr(2, 64),
        },
      },
    ];

    await this.setupMockResponses(defiMocks);
  }

  // Governance service mocks
  private async setupDefaultGovernanceMocks(): Promise<void> {
    const governanceMocks: MockAPIResponse[] = [
      {
        endpoint: '**/api/governance/proposals',
        method: 'GET',
        response: [
          {
            id: '1',
            title: 'Increase Weather Data Rewards',
            description:
              'Proposal to increase base rewards for weather data providers by 25%',
            status: 'active',
            votes: { for: '1250000', against: '350000', abstain: '100000' },
            endTime: new Date(
              Date.now() + 6 * 24 * 60 * 60 * 1000
            ).toISOString(),
            proposer: '0x' + Math.random().toString(16).substr(2, 40),
          },
          {
            id: '2',
            title: 'Carbon Credit Integration Upgrade',
            description:
              'Upgrade carbon credit verification system with new standards',
            status: 'passed',
            votes: { for: '2100000', against: '400000', abstain: '200000' },
            endTime: new Date(Date.now() - 24 * 60 * 60 * 1000).toISOString(),
            proposer: '0x' + Math.random().toString(16).substr(2, 40),
          },
        ],
      },
      {
        endpoint: '**/api/governance/vote',
        method: 'POST',
        response: {
          success: true,
          voteId: 'vote_' + Math.random().toString(36).substr(2, 9),
          votingPower: '1000',
          transactionHash: '0x' + Math.random().toString(16).substr(2, 64),
        },
      },
      {
        endpoint: '**/api/governance/delegate',
        method: 'POST',
        response: {
          success: true,
          delegationId: 'delegation_' + Math.random().toString(36).substr(2, 9),
          delegatedPower: '5000',
          transactionHash: '0x' + Math.random().toString(16).substr(2, 64),
        },
      },
    ];

    await this.setupMockResponses(governanceMocks);
  }

  // Cross-chain bridge mocks
  private async setupDefaultBridgeMocks(): Promise<void> {
    const bridgeMocks: MockAPIResponse[] = [
      {
        endpoint: '**/api/bridge/supported-chains',
        method: 'GET',
        response: [
          {
            id: 'ethereum',
            name: 'Ethereum',
            chainId: 1,
            icon: '/icons/ethereum.svg',
          },
          {
            id: 'polygon',
            name: 'Polygon',
            chainId: 137,
            icon: '/icons/polygon.svg',
          },
          {
            id: 'bsc',
            name: 'Binance Smart Chain',
            chainId: 56,
            icon: '/icons/bsc.svg',
          },
          {
            id: 'avalanche',
            name: 'Avalanche',
            chainId: 43114,
            icon: '/icons/avalanche.svg',
          },
        ],
      },
      {
        endpoint: '**/api/bridge/quote',
        method: 'POST',
        response: {
          sourceChain: 'ethereum',
          destinationChain: 'polygon',
          sourceAmount: '100',
          destinationAmount: '99.5',
          bridgeFee: '0.5',
          estimatedTime: '5-10 minutes',
          route: [
            { step: 'Lock tokens on Ethereum', estimated: '2-3 minutes' },
            { step: 'Validate on bridge', estimated: '1-2 minutes' },
            { step: 'Mint on Polygon', estimated: '2-5 minutes' },
          ],
        },
      },
      {
        endpoint: '**/api/bridge/transfer',
        method: 'POST',
        response: {
          bridgeId: 'bridge_' + Math.random().toString(36).substr(2, 9),
          status: 'initiated',
          sourceTransaction: '0x' + Math.random().toString(16).substr(2, 64),
          estimatedCompletion: new Date(
            Date.now() + 10 * 60 * 1000
          ).toISOString(),
          trackingUrl:
            '/bridge/track/bridge_' + Math.random().toString(36).substr(2, 9),
        },
      },
      {
        endpoint: '**/api/bridge/status/**',
        method: 'GET',
        response: {
          status: 'completed',
          progress: 100,
          sourceTransaction: '0x' + Math.random().toString(16).substr(2, 64),
          destinationTransaction:
            '0x' + Math.random().toString(16).substr(2, 64),
          completedAt: new Date().toISOString(),
        },
      },
    ];

    await this.setupMockResponses(bridgeMocks);
  }

  // IoT device mocks
  private async setupDefaultIoTMocks(): Promise<void> {
    const iotMocks: MockAPIResponse[] = [
      {
        endpoint: '**/api/iot/devices',
        method: 'GET',
        response: [
          {
            id: 'device_001',
            name: 'Weather Station NYC 1',
            type: 'weather_station',
            status: 'active',
            location: { lat: 40.7128, lon: -74.006, name: 'New York City' },
            lastReading: new Date(Date.now() - 300000).toISOString(),
            dataQuality: 0.95,
          },
          {
            id: 'device_002',
            name: 'Weather Station NYC 2',
            type: 'weather_station',
            status: 'active',
            location: { lat: 40.7589, lon: -73.9851, name: 'Central Park' },
            lastReading: new Date(Date.now() - 180000).toISOString(),
            dataQuality: 0.88,
          },
        ],
      },
      {
        endpoint: '**/api/iot/device/register',
        method: 'POST',
        response: {
          success: true,
          deviceId: 'device_' + Math.random().toString(36).substr(2, 9),
          apiKey: 'api_' + Math.random().toString(36).substr(2, 32),
          webhookUrl: 'https://api.wrfcoin.com/webhook/device_data',
          configuration: {
            dataInterval: 300,
            qualityThreshold: 0.8,
            retryAttempts: 3,
          },
        },
      },
      {
        endpoint: '**/api/iot/device/*/data',
        method: 'POST',
        response: {
          received: true,
          dataId: 'data_' + Math.random().toString(36).substr(2, 9),
          qualityScore: 0.85 + Math.random() * 0.15,
          validation: {
            passed: true,
            issues: [],
          },
          rewards: {
            earned: (0.1 + Math.random() * 0.5).toFixed(3),
            currency: 'WRF',
          },
        },
      },
    ];

    await this.setupMockResponses(iotMocks);
  }

  // Setup mock responses helper
  private async setupMockResponses(mocks: MockAPIResponse[]): Promise<void> {
    for (const mock of mocks) {
      const mockId = `${mock.method}_${mock.endpoint}`;

      const route = await this.page.route(mock.endpoint, async route => {
        if (route.request().method() !== mock.method) {
          route.continue();
          return;
        }

        // Check conditional logic if provided
        if (mock.conditional) {
          const requestData = route.request().postDataJSON();
          if (!mock.conditional(requestData)) {
            route.continue();
            return;
          }
        }

        // Add delay if specified
        if (mock.delay) {
          await new Promise(resolve => setTimeout(resolve, mock.delay));
        }

        // Fulfill with mock response
        await route.fulfill({
          status: mock.status || 200,
          contentType: 'application/json',
          body: JSON.stringify(mock.response),
        });
      });

      this.activeMocks.set(mockId, route);
    }
  }

  // Simulate API failures
  async simulateFailure(
    endpoint: string,
    method: string,
    errorType: 'timeout' | 'server-error' | 'network-error' = 'server-error'
  ): Promise<void> {
    const mockId = `${method}_${endpoint}`;

    await this.page.route(endpoint, async route => {
      if (route.request().method() !== method) {
        route.continue();
        return;
      }

      switch (errorType) {
        case 'timeout':
          // Simulate timeout by delaying response
          await new Promise(resolve => setTimeout(resolve, 30000));
          await route.abort('timedout');
          break;
        case 'server-error':
          await route.fulfill({
            status: 500,
            contentType: 'application/json',
            body: JSON.stringify({
              error: 'Internal Server Error',
              message: 'Something went wrong on the server',
            }),
          });
          break;
        case 'network-error':
          await route.abort('internetdisconnected');
          break;
      }
    });

    this.activeMocks.set(`${mockId}_failure`, route);
  }

  // Simulate rate limiting
  async simulateRateLimit(endpoint: string, method: string): Promise<void> {
    const mockId = `${method}_${endpoint}_ratelimit`;

    const route = await this.page.route(endpoint, async route => {
      if (route.request().method() !== method) {
        route.continue();
        return;
      }

      await route.fulfill({
        status: 429,
        contentType: 'application/json',
        headers: {
          'X-RateLimit-Limit': '1000',
          'X-RateLimit-Remaining': '0',
          'X-RateLimit-Reset': String(Math.floor(Date.now() / 1000) + 3600),
        },
        body: JSON.stringify({
          error: 'Rate limit exceeded',
          message: 'Too many requests, please try again later',
        }),
      });
    });

    this.activeMocks.set(mockId, route);
  }

  // Clear all mocks
  async clearAllMocks(): Promise<void> {
    for (const [mockId, route] of this.activeMocks) {
      try {
        await this.page.unroute(mockId);
      } catch (error) {
        console.warn(`Failed to clear mock ${mockId}:`, error);
      }
    }
    this.activeMocks.clear();
  }

  // Clear specific mock
  async clearMock(endpoint: string, method: string): Promise<void> {
    const mockId = `${method}_${endpoint}`;
    const route = this.activeMocks.get(mockId);
    if (route) {
      await this.page.unroute(endpoint);
      this.activeMocks.delete(mockId);
    }
  }

  // Get active mocks for debugging
  getActiveMocks(): string[] {
    return Array.from(this.activeMocks.keys());
  }

  // Verify mock was called
  async verifyMockCalled(endpoint: string, method: string): Promise<boolean> {
    const mockId = `${method}_${endpoint}`;
    return this.activeMocks.has(mockId);
  }

  // Create custom mock
  async createCustomMock(
    endpoint: string,
    method: 'GET' | 'POST' | 'PUT' | 'DELETE',
    handler: (route: Route) => Promise<void>
  ): Promise<void> {
    const mockId = `${method}_${endpoint}_custom`;

    const route = await this.page.route(endpoint, async route => {
      if (route.request().method() !== method) {
        route.continue();
        return;
      }
      await handler(route);
    });

    this.activeMocks.set(mockId, route);
  }
}

export default MockServiceManager;
