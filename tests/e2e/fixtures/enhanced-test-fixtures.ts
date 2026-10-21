import { test as base, expect } from '@playwright/test';
import { WalletPage } from '../page-objects/wallet/wallet-page';
import { WeatherDataPage } from '../page-objects/weather/weather-data-page';
import { RewardsPage } from '../page-objects/rewards/rewards-page';
import { APIHelper } from '../utils/test-helpers';
import { WeatherDataGenerator } from '../utils/weather-data-generator';
import { RewardCalculator } from '../utils/reward-calculator';

// Enhanced fixtures with all required page objects and utilities
type EnhancedTestFixtures = {
  walletPage: WalletPage;
  weatherDataPage: WeatherDataPage;
  rewardsPage: RewardsPage;
  apiHelper: APIHelper;
  weatherDataGenerator: WeatherDataGenerator;
  rewardCalculator: RewardCalculator;
  authenticatedUser: {
    token: string;
    address: string;
    userId: string;
    role: string;
  };
  testEnvironment: {
    baseUrls: Record<string, string>;
    timeouts: Record<string, number>;
    testData: Record<string, any>;
  };
};

export const test = base.extend<EnhancedTestFixtures>({
  // Core page objects
  walletPage: async ({ page }, use) => {
    const walletPage = new WalletPage(page);
    await use(walletPage);
  },

  weatherDataPage: async ({ page }, use) => {
    const weatherDataPage = new WeatherDataPage(page);
    await use(weatherDataPage);
  },

  rewardsPage: async ({ page }, use) => {
    const rewardsPage = new RewardsPage(page);
    await use(rewardsPage);
  },

  // API and utilities
  apiHelper: async ({}, use) => {
    const apiHelper = new APIHelper();
    await use(apiHelper);
  },

  weatherDataGenerator: async ({}, use) => {
    const generator = new WeatherDataGenerator();
    await use(generator);
  },

  rewardCalculator: async ({}, use) => {
    const calculator = new RewardCalculator();
    await use(calculator);
  },

  // Enhanced authenticated user fixture
  authenticatedUser: async ({ apiHelper }, use) => {
    const testCredentials = {
      username: `test_user_${Date.now()}@wrfcoin.test`,
      password: 'TestPassword123!',
      role: 'weather_provider',
    };

    try {
      // Create test user via API
      const createUserResponse = await fetch(
        `${process.env.MAIN_API_URL || 'http://localhost:4001'}/api/auth/register`,
        {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({
            ...testCredentials,
            walletAddress: '0x742d35Cc6634C0532925a3b8D1c9e37C2384C24D',
          }),
        }
      );

      if (createUserResponse.ok) {
        const userData = await createUserResponse.json();

        // Authenticate user
        const token = await apiHelper.authenticate({
          username: testCredentials.username,
          password: testCredentials.password,
        });

        await use({
          token,
          address: '0x742d35Cc6634C0532925a3b8D1c9e37C2384C24D',
          userId: userData.userId,
          role: testCredentials.role,
        });

        // Cleanup: Delete test user after test
        try {
          await fetch(
            `${process.env.MAIN_API_URL || 'http://localhost:4001'}/api/users/${userData.userId}`,
            {
              method: 'DELETE',
              headers: {
                Authorization: `Bearer ${token}`,
                'Content-Type': 'application/json',
              },
            }
          );
        } catch (error) {
          console.warn('Failed to cleanup test user:', error);
        }
      } else {
        // Fallback to mock user
        await use({
          token: 'mock_token',
          address: '0x742d35Cc6634C0532925a3b8D1c9e37C2384C24D',
          userId: 'mock_user_id',
          role: 'weather_provider',
        });
      }
    } catch (error) {
      console.warn('Authentication failed, using mock user:', error);
      await use({
        token: 'mock_token',
        address: '0x742d35Cc6634C0532925a3b8D1c9e37C2384C24D',
        userId: 'mock_user_id',
        role: 'weather_provider',
      });
    }
  },

  // Test environment configuration
  testEnvironment: async ({}, use) => {
    const environment = {
      baseUrls: {
        frontend: process.env.E2E_FRONTEND_URL || 'http://localhost:5000',
        mainApi: process.env.MAIN_API_URL || 'http://localhost:4001',
        walletServer: process.env.WALLET_SERVER_URL || 'http://localhost:4000',
        iotNetworks:
          process.env.IOT_SENSOR_NETWORKS_URL || 'http://localhost:3002',
        computeNetworks:
          process.env.COMPUTE_NETWORKS_URL || 'http://localhost:6000',
        crossChain: process.env.CROSS_CHAIN_URL || 'http://localhost:3000',
        governance:
          process.env.GOVERNANCE_SERVER_URL || 'http://localhost:8000',
      },
      timeouts: {
        pageLoad: 30000,
        apiResponse: 15000,
        transactionConfirmation: 60000,
        rewardProcessing: 30000,
        dataValidation: 10000,
      },
      testData: {
        defaultWeatherStation: {
          name: 'Test Weather Station',
          location: 'New York, NY',
          lat: 40.7128,
          lon: -74.006,
          type: 'ambient_weather',
        },
        rewardMultipliers: {
          highQuality: 1.2,
          mediumQuality: 1.0,
          lowQuality: 0.8,
          sparseRegion: 1.5,
          denseRegion: 1.0,
          frequencyBonus: 0.1,
        },
        testTokens: {
          WRF: {
            address: '0x1234567890123456789012345678901234567890',
            symbol: 'WRF',
            decimals: 18,
          },
          CARBON: {
            address: '0x2345678901234567890123456789012345678901',
            symbol: 'CARBON',
            decimals: 18,
          },
        },
        networkConfig: {
          chainId: 31337,
          rpcUrl: 'http://localhost:8545',
          blockTime: 1000, // 1 second for testing
        },
      },
    };

    await use(environment);
  },
});

// Enhanced worker-scoped fixtures for global setup
type WorkerFixtures = {
  serviceHealthCheck: {
    mainApi: boolean;
    walletServer: boolean;
    iotNetworks: boolean;
    computeNetworks: boolean;
  };
  testDataCleanup: () => Promise<void>;
};

export const workerTest = base.extend<{}, WorkerFixtures>({
  // Service health check
  serviceHealthCheck: [
    async ({}, use) => {
      const services = {
        mainApi: false,
        walletServer: false,
        iotNetworks: false,
        computeNetworks: false,
      };

      // Check service availability
      const checkService = async (
        name: string,
        url: string
      ): Promise<boolean> => {
        try {
          const response = await fetch(`${url}/health`, {
            method: 'GET',
            timeout: 5000,
          });
          return response.ok;
        } catch (error) {
          console.warn(`Service ${name} health check failed:`, error);
          return false;
        }
      };

      services.mainApi = await checkService(
        'Main API',
        process.env.MAIN_API_URL || 'http://localhost:4001'
      );
      services.walletServer = await checkService(
        'Wallet Server',
        process.env.WALLET_SERVER_URL || 'http://localhost:4000'
      );
      services.iotNetworks = await checkService(
        'IoT Networks',
        process.env.IOT_SENSOR_NETWORKS_URL || 'http://localhost:3002'
      );
      services.computeNetworks = await checkService(
        'Compute Networks',
        process.env.COMPUTE_NETWORKS_URL || 'http://localhost:6000'
      );

      console.log('Service Health Status:', services);

      await use(services);
    },
    { scope: 'worker' },
  ],

  // Test data cleanup
  testDataCleanup: [
    async ({}, use) => {
      const cleanup = async () => {
        try {
          // Cleanup test users, devices, submissions, etc.
          const apiHelper = new APIHelper();

          // Clean up test IoT devices
          const testDevices = await apiHelper.getTestDevices();
          for (const device of testDevices) {
            await apiHelper.deleteDevice(device.id);
          }

          // Clean up test weather submissions
          const testSubmissions = await apiHelper.getTestSubmissions();
          for (const submission of testSubmissions) {
            await apiHelper.deleteSubmission(submission.id);
          }

          // Clean up test users
          const testUsers = await apiHelper.getTestUsers();
          for (const user of testUsers) {
            await apiHelper.deleteUser(user.id);
          }

          console.log('Test data cleanup completed');
        } catch (error) {
          console.warn('Test data cleanup failed:', error);
        }
      };

      await use(cleanup);
    },
    { scope: 'worker' },
  ],
});

export { expect } from '@playwright/test';

// Helper types for test data
export interface WeatherDataSubmission {
  deviceId: string;
  timestamp: string;
  data: {
    temperature: number;
    humidity: number;
    pressure: number;
    windSpeed: number;
  };
  location: {
    lat: number;
    lon: number;
    name: string;
  };
  quality: {
    accuracy: number;
    completeness: number;
    timeliness: number;
  };
}

export interface RewardCalculation {
  baseReward: number;
  qualityBonus: number;
  frequencyBonus: number;
  geographicBonus: number;
  verificationBonus?: number;
  totalReward: number;
}

export interface TestWalletData {
  address: string;
  privateKey: string;
  mnemonic: string;
  balance?: {
    WRF: number;
    ETH: number;
  };
}

export interface IoTDeviceConfig {
  id: string;
  name: string;
  type: string;
  location: string;
  apiKey: string;
  collectionInterval: number;
  dataTypes: string[];
  status: 'active' | 'inactive' | 'error';
}

export interface TestScenario {
  name: string;
  description: string;
  setup: () => Promise<void>;
  teardown: () => Promise<void>;
  data: Record<string, any>;
}

// Common test scenarios
export const TEST_SCENARIOS = {
  HIGH_QUALITY_DATA_PROVIDER: {
    name: 'High Quality Data Provider',
    description: 'Weather station providing consistent, high-quality data',
    data: {
      qualityScore: 0.95,
      submissionFrequency: 300, // 5 minutes
      location: { lat: 40.7128, lon: -74.006, name: 'New York, NY' },
      expectedMultiplier: 1.2,
    },
  },

  SPARSE_REGION_PROVIDER: {
    name: 'Sparse Region Provider',
    description: 'Weather station in data-sparse geographic region',
    data: {
      qualityScore: 0.85,
      submissionFrequency: 600, // 10 minutes
      location: { lat: 71.0489, lon: -156.7889, name: 'Utqiagvik, Alaska' },
      expectedMultiplier: 1.5,
    },
  },

  FREQUENT_SUBMITTER: {
    name: 'Frequent Data Submitter',
    description: 'Provider with very frequent data submissions',
    data: {
      qualityScore: 0.9,
      submissionFrequency: 60, // 1 minute
      location: { lat: 37.7749, lon: -122.4194, name: 'San Francisco, CA' },
      expectedFrequencyBonus: 0.15,
    },
  },

  BATCH_DATA_PROCESSOR: {
    name: 'Batch Data Processor',
    description: 'Provider uploading historical data in batches',
    data: {
      qualityScore: 0.8,
      batchSize: 1000,
      dataRange: '30d',
      expectedProcessingTime: 60000, // 60 seconds
    },
  },

  CROSS_SERVICE_INTEGRATOR: {
    name: 'Cross-Service Integrator',
    description: 'Provider contributing to compute jobs and carbon credits',
    data: {
      qualityScore: 0.92,
      computeJobsEnabled: true,
      carbonCreditGeneration: true,
      stakingPoolAccess: true,
    },
  },
};
