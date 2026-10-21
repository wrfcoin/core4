// WRFCoin Test Data Management System
import { Page } from '@playwright/test';
import crypto from 'crypto';

export interface TestDataCategory {
  users: TestUser[];
  wallets: TestWallet[];
  weatherData: WeatherData[];
  financialData: FinancialData[];
  governanceData: GovernanceData[];
  nftData: NFTData[];
  iotDevices: IoTDevice[];
  crossChainData: CrossChainData[];
}

export interface TestUser {
  id: string;
  email: string;
  password: string;
  role: 'admin' | 'user' | 'validator' | 'meteorologist' | 'trader';
  profile: {
    name: string;
    location: string;
    experienceLevel: 'beginner' | 'intermediate' | 'advanced' | 'expert';
    preferences: Record<string, any>;
  };
  authTokens: {
    accessToken?: string;
    refreshToken?: string;
    apiKey?: string;
  };
  createdAt: string;
  lastLogin?: string;
}

export interface TestWallet {
  id: string;
  address: string;
  privateKey: string;
  mnemonic: string;
  chainId: number;
  networkName: string;
  balances: {
    [token: string]: string;
  };
  transactions: Transaction[];
  type: 'metamask' | 'walletconnect' | 'platform' | 'hardware';
}

export interface WeatherData {
  id: string;
  deviceId: string;
  timestamp: string;
  location: {
    lat: number;
    lon: number;
    altitude?: number;
  };
  measurements: {
    temperature: number;
    humidity: number;
    pressure: number;
    windSpeed: number;
    windDirection: number;
    rainfall?: number;
    solarRadiation?: number;
  };
  qualityScore: number;
  verified: boolean;
}

export interface FinancialData {
  id: string;
  userId: string;
  walletAddress: string;
  positions: {
    liquidityPools: LiquidityPosition[];
    stakingPositions: StakingPosition[];
    yieldFarms: YieldFarmPosition[];
  };
  transactionHistory: FinancialTransaction[];
  totalValue: number;
  pnl: number;
}

export interface GovernanceData {
  id: string;
  proposals: GovernanceProposal[];
  votes: Vote[];
  delegations: Delegation[];
  daoMembers: DAOMember[];
}

export interface NFTData {
  id: string;
  tokenId: string;
  contractAddress: string;
  metadata: {
    name: string;
    description: string;
    image: string;
    attributes: Array<{
      trait_type: string;
      value: string | number;
    }>;
  };
  owner: string;
  creator: string;
  marketData: {
    currentPrice?: number;
    lastSalePrice?: number;
    floorPrice?: number;
  };
}

export interface IoTDevice {
  id: string;
  serialNumber: string;
  type: string;
  model: string;
  manufacturer: string;
  location: {
    lat: number;
    lon: number;
    name: string;
  };
  status: 'active' | 'inactive' | 'maintenance' | 'error';
  lastDataTransmission: string;
  configuration: Record<string, any>;
  owner: string;
}

export interface CrossChainData {
  id: string;
  bridges: BridgeTransaction[];
  crossChainAssets: CrossChainAsset[];
  supportedChains: Chain[];
}

// Additional interfaces
interface Transaction {
  id: string;
  hash: string;
  from: string;
  to: string;
  value: string;
  gasUsed: string;
  timestamp: string;
  status: 'pending' | 'confirmed' | 'failed';
}

interface LiquidityPosition {
  poolId: string;
  token0: string;
  token1: string;
  amount0: string;
  amount1: string;
  lpTokens: string;
  apy: number;
}

interface StakingPosition {
  stakingPool: string;
  stakedAmount: string;
  rewardsEarned: string;
  lockupPeriod: number;
  maturityDate: string;
}

interface YieldFarmPosition {
  farmId: string;
  stakedTokens: string;
  rewardTokens: string;
  apy: number;
  harvestedRewards: string;
}

interface FinancialTransaction {
  id: string;
  type: 'swap' | 'liquidity' | 'stake' | 'harvest' | 'bridge';
  amount: string;
  token: string;
  timestamp: string;
  gasUsed: string;
  status: string;
}

interface GovernanceProposal {
  id: string;
  title: string;
  description: string;
  proposer: string;
  status: 'active' | 'passed' | 'rejected' | 'executed';
  votingPower: string;
  votes: { for: string; against: string; abstain: string };
  startTime: string;
  endTime: string;
}

interface Vote {
  id: string;
  proposalId: string;
  voter: string;
  choice: 'for' | 'against' | 'abstain';
  votingPower: string;
  timestamp: string;
}

interface Delegation {
  id: string;
  delegator: string;
  delegate: string;
  votingPower: string;
  timestamp: string;
}

interface DAOMember {
  id: string;
  address: string;
  votingPower: string;
  reputation: number;
  proposalsCreated: number;
  votesParticipated: number;
  joinDate: string;
}

interface BridgeTransaction {
  id: string;
  sourceChain: string;
  destinationChain: string;
  sourceToken: string;
  destinationToken: string;
  amount: string;
  bridgeProvider: string;
  status: 'pending' | 'completed' | 'failed';
  fees: string;
  timestamp: string;
}

interface CrossChainAsset {
  id: string;
  symbol: string;
  name: string;
  supportedChains: string[];
  totalSupply: string;
  bridgeable: boolean;
}

interface Chain {
  id: string;
  name: string;
  chainId: number;
  rpcUrl: string;
  blockExplorer: string;
  nativeCurrency: {
    name: string;
    symbol: string;
    decimals: number;
  };
}

export class TestDataManager {
  private static instance: TestDataManager;
  private dataStore: Map<string, TestDataCategory> = new Map();
  private cleanupTasks: Array<() => Promise<void>> = [];

  static getInstance(): TestDataManager {
    if (!TestDataManager.instance) {
      TestDataManager.instance = new TestDataManager();
    }
    return TestDataManager.instance;
  }

  // Generate test data for specific scenarios
  async generateTestData(
    scenario: string,
    options?: any
  ): Promise<TestDataCategory> {
    const testId = `${scenario}_${Date.now()}_${crypto.randomBytes(4).toString('hex')}`;

    const testData: TestDataCategory = {
      users: await this.generateUsers(scenario, options),
      wallets: await this.generateWallets(scenario, options),
      weatherData: await this.generateWeatherData(scenario, options),
      financialData: await this.generateFinancialData(scenario, options),
      governanceData: await this.generateGovernanceData(scenario, options),
      nftData: await this.generateNFTData(scenario, options),
      iotDevices: await this.generateIoTDevices(scenario, options),
      crossChainData: await this.generateCrossChainData(scenario, options),
    };

    this.dataStore.set(testId, testData);
    return testData;
  }

  // Seed data into the application
  async seedData(page: Page, testData: TestDataCategory): Promise<void> {
    console.log('Seeding test data into application...');

    // Seed users
    for (const user of testData.users) {
      await this.seedUser(page, user);
    }

    // Seed wallets
    for (const wallet of testData.wallets) {
      await this.seedWallet(page, wallet);
    }

    // Seed weather data
    for (const weatherData of testData.weatherData) {
      await this.seedWeatherData(page, weatherData);
    }

    // Seed financial data
    for (const financialData of testData.financialData) {
      await this.seedFinancialData(page, financialData);
    }

    // Seed IoT devices
    for (const device of testData.iotDevices) {
      await this.seedIoTDevice(page, device);
    }

    console.log('Test data seeding completed');
  }

  // Cleanup test data
  async cleanup(testId?: string): Promise<void> {
    if (testId) {
      this.dataStore.delete(testId);
    } else {
      this.dataStore.clear();
    }

    // Execute cleanup tasks
    for (const cleanupTask of this.cleanupTasks) {
      try {
        await cleanupTask();
      } catch (error) {
        console.error('Cleanup task failed:', error);
      }
    }
    this.cleanupTasks = [];
  }

  // Validate data consistency
  async validateData(page: Page, testData: TestDataCategory): Promise<boolean> {
    console.log('Validating test data consistency...');

    // Validate users exist and are accessible
    for (const user of testData.users) {
      const isValid = await this.validateUser(page, user);
      if (!isValid) {
        console.error(`User validation failed for: ${user.email}`);
        return false;
      }
    }

    // Validate wallets are connected and have balances
    for (const wallet of testData.wallets) {
      const isValid = await this.validateWallet(page, wallet);
      if (!isValid) {
        console.error(`Wallet validation failed for: ${wallet.address}`);
        return false;
      }
    }

    console.log('Data validation completed successfully');
    return true;
  }

  // Generate realistic weather data
  private async generateWeatherData(
    scenario: string,
    options?: any
  ): Promise<WeatherData[]> {
    const weatherData: WeatherData[] = [];
    const numRecords = options?.numRecords || 100;

    for (let i = 0; i < numRecords; i++) {
      const baseTime = Date.now() - i * 300000; // Every 5 minutes

      weatherData.push({
        id: `weather_${i}_${crypto.randomBytes(4).toString('hex')}`,
        deviceId: `device_${Math.floor(i / 10)}_${crypto.randomBytes(2).toString('hex')}`,
        timestamp: new Date(baseTime).toISOString(),
        location: {
          lat: 40.7128 + (Math.random() - 0.5) * 0.1,
          lon: -74.006 + (Math.random() - 0.5) * 0.1,
          altitude: Math.random() * 100,
        },
        measurements: {
          temperature: 15 + Math.random() * 20, // 15-35°C
          humidity: 30 + Math.random() * 40, // 30-70%
          pressure: 980 + Math.random() * 60, // 980-1040 hPa
          windSpeed: Math.random() * 25, // 0-25 m/s
          windDirection: Math.random() * 360, // 0-360 degrees
          rainfall: Math.random() < 0.3 ? Math.random() * 10 : 0,
          solarRadiation: Math.random() * 1000,
        },
        qualityScore: 0.7 + Math.random() * 0.3, // 70-100%
        verified: Math.random() > 0.1, // 90% verified
      });
    }

    return weatherData;
  }

  // Generate test users
  private async generateUsers(
    scenario: string,
    options?: any
  ): Promise<TestUser[]> {
    const users: TestUser[] = [];
    const userTypes = ['user', 'validator', 'meteorologist', 'trader'] as const;

    for (let i = 0; i < (options?.numUsers || 5); i++) {
      const userId = crypto.randomBytes(8).toString('hex');
      const userType = userTypes[i % userTypes.length];

      users.push({
        id: userId,
        email: `testuser_${userId}@test.wrfcoin.com`,
        password: 'TestPassword123!',
        role: userType,
        profile: {
          name: `Test User ${i + 1}`,
          location: `Test Location ${i + 1}`,
          experienceLevel: ['beginner', 'intermediate', 'advanced', 'expert'][
            Math.floor(Math.random() * 4)
          ] as any,
          preferences: {
            theme: 'dark',
            notifications: true,
            language: 'en',
          },
        },
        authTokens: {},
        createdAt: new Date().toISOString(),
      });
    }

    return users;
  }

  // Generate test wallets
  private async generateWallets(
    scenario: string,
    options?: any
  ): Promise<TestWallet[]> {
    const wallets: TestWallet[] = [];
    const numWallets = options?.numWallets || 3;

    for (let i = 0; i < numWallets; i++) {
      const privateKey = '0x' + crypto.randomBytes(32).toString('hex');
      const address = '0x' + crypto.randomBytes(20).toString('hex');

      wallets.push({
        id: `wallet_${i}`,
        address,
        privateKey,
        mnemonic: 'test test test test test test test test test test test junk',
        chainId: 31337,
        networkName: 'localhost',
        balances: {
          WRF: (1000 + Math.random() * 9000).toString(),
          ETH: (1 + Math.random() * 9).toString(),
          CARBON: (500 + Math.random() * 1500).toString(),
        },
        transactions: [],
        type: ['metamask', 'walletconnect', 'platform'][
          Math.floor(Math.random() * 3)
        ] as any,
      });
    }

    return wallets;
  }

  // Generate financial data
  private async generateFinancialData(
    scenario: string,
    options?: any
  ): Promise<FinancialData[]> {
    const financialData: FinancialData[] = [];
    const numAccounts = options?.numAccounts || 3;

    for (let i = 0; i < numAccounts; i++) {
      financialData.push({
        id: `financial_${i}`,
        userId: `user_${i}`,
        walletAddress: `0x${crypto.randomBytes(20).toString('hex')}`,
        positions: {
          liquidityPools: [
            {
              poolId: 'WRF-ETH',
              token0: 'WRF',
              token1: 'ETH',
              amount0: (100 + Math.random() * 900).toString(),
              amount1: (1 + Math.random() * 9).toString(),
              lpTokens: (50 + Math.random() * 450).toString(),
              apy: 0.05 + Math.random() * 0.25,
            },
          ],
          stakingPositions: [
            {
              stakingPool: 'WRF-STAKE',
              stakedAmount: (500 + Math.random() * 1500).toString(),
              rewardsEarned: (10 + Math.random() * 90).toString(),
              lockupPeriod: 30,
              maturityDate: new Date(
                Date.now() + 30 * 24 * 60 * 60 * 1000
              ).toISOString(),
            },
          ],
          yieldFarms: [],
        },
        transactionHistory: [],
        totalValue: 10000 + Math.random() * 90000,
        pnl: -1000 + Math.random() * 2000,
      });
    }

    return financialData;
  }

  // Generate governance data
  private async generateGovernanceData(
    scenario: string,
    options?: any
  ): Promise<GovernanceData[]> {
    return [
      {
        id: 'governance_main',
        proposals: [
          {
            id: 'proposal_1',
            title: 'Increase Weather Data Rewards',
            description:
              'Proposal to increase rewards for high-quality weather data',
            proposer: '0x' + crypto.randomBytes(20).toString('hex'),
            status: 'active',
            votingPower: '1000000',
            votes: { for: '600000', against: '300000', abstain: '100000' },
            startTime: new Date(Date.now() - 86400000).toISOString(),
            endTime: new Date(Date.now() + 6 * 86400000).toISOString(),
          },
        ],
        votes: [],
        delegations: [],
        daoMembers: [],
      },
    ];
  }

  // Generate NFT data
  private async generateNFTData(
    scenario: string,
    options?: any
  ): Promise<NFTData[]> {
    const nftData: NFTData[] = [];
    const numNFTs = options?.numNFTs || 5;

    for (let i = 0; i < numNFTs; i++) {
      nftData.push({
        id: `nft_${i}`,
        tokenId: i.toString(),
        contractAddress: '0x' + crypto.randomBytes(20).toString('hex'),
        metadata: {
          name: `Weather NFT #${i}`,
          description: `A computational NFT representing weather data analysis #${i}`,
          image: `https://picsum.photos/400/400?random=${i}`,
          attributes: [
            {
              trait_type: 'Rarity',
              value: ['Common', 'Uncommon', 'Rare', 'Epic'][
                Math.floor(Math.random() * 4)
              ],
            },
            {
              trait_type: 'Data Quality',
              value: Math.floor(70 + Math.random() * 30),
            },
            { trait_type: 'Location', value: `Location ${i}` },
          ],
        },
        owner: '0x' + crypto.randomBytes(20).toString('hex'),
        creator: '0x' + crypto.randomBytes(20).toString('hex'),
        marketData: {
          currentPrice: 0.1 + Math.random() * 0.9,
          lastSalePrice: 0.1 + Math.random() * 0.9,
          floorPrice: 0.05 + Math.random() * 0.5,
        },
      });
    }

    return nftData;
  }

  // Generate IoT devices
  private async generateIoTDevices(
    scenario: string,
    options?: any
  ): Promise<IoTDevice[]> {
    const devices: IoTDevice[] = [];
    const numDevices = options?.numDevices || 10;
    const manufacturers = [
      'Acurite',
      'WeatherFlow',
      'Ambient Weather',
      'Davis Instruments',
    ];
    const models = ['Atlas', 'Tempest', 'WS-2902', 'Vantage Pro2'];

    for (let i = 0; i < numDevices; i++) {
      const manufacturer =
        manufacturers[Math.floor(Math.random() * manufacturers.length)];
      const model = models[Math.floor(Math.random() * models.length)];

      devices.push({
        id: `device_${i}`,
        serialNumber: `${manufacturer.toUpperCase()}-${model.toUpperCase()}-${crypto.randomBytes(4).toString('hex')}`,
        type: 'weather_station',
        model,
        manufacturer,
        location: {
          lat: 40.7128 + (Math.random() - 0.5) * 0.2,
          lon: -74.006 + (Math.random() - 0.5) * 0.2,
          name: `Test Location ${i + 1}`,
        },
        status: ['active', 'inactive', 'maintenance'][
          Math.floor(Math.random() * 3)
        ] as any,
        lastDataTransmission: new Date(
          Date.now() - Math.random() * 3600000
        ).toISOString(),
        configuration: {
          dataInterval: 300, // 5 minutes
          qualityThreshold: 0.8,
          autoCalibration: true,
        },
        owner: `user_${Math.floor(Math.random() * 5)}`,
      });
    }

    return devices;
  }

  // Generate cross-chain data
  private async generateCrossChainData(
    scenario: string,
    options?: any
  ): Promise<CrossChainData[]> {
    return [
      {
        id: 'crosschain_main',
        bridges: [
          {
            id: 'bridge_1',
            sourceChain: 'ethereum',
            destinationChain: 'polygon',
            sourceToken: 'WRF',
            destinationToken: 'WRF',
            amount: '100',
            bridgeProvider: 'WRFCoin Bridge',
            status: 'completed',
            fees: '0.01',
            timestamp: new Date().toISOString(),
          },
        ],
        crossChainAssets: [
          {
            id: 'wrf_cross_chain',
            symbol: 'WRF',
            name: 'WRFCoin Token',
            supportedChains: ['ethereum', 'polygon', 'bsc', 'avalanche'],
            totalSupply: '1000000000',
            bridgeable: true,
          },
        ],
        supportedChains: [
          {
            id: 'ethereum',
            name: 'Ethereum',
            chainId: 1,
            rpcUrl: 'https://mainnet.infura.io/v3/test',
            blockExplorer: 'https://etherscan.io',
            nativeCurrency: { name: 'Ethereum', symbol: 'ETH', decimals: 18 },
          },
          {
            id: 'polygon',
            name: 'Polygon',
            chainId: 137,
            rpcUrl: 'https://polygon-rpc.com',
            blockExplorer: 'https://polygonscan.com',
            nativeCurrency: { name: 'MATIC', symbol: 'MATIC', decimals: 18 },
          },
        ],
      },
    ];
  }

  // Validation methods
  private async validateUser(page: Page, user: TestUser): Promise<boolean> {
    try {
      // Check if user exists in system
      await page.goto('/api/users/verify');
      const response = await page.evaluate(async email => {
        return fetch('/api/users/verify', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ email }),
        }).then(r => r.ok);
      }, user.email);
      return response;
    } catch {
      return false;
    }
  }

  private async validateWallet(
    page: Page,
    wallet: TestWallet
  ): Promise<boolean> {
    try {
      // Verify wallet is accessible
      const isValid = await page.evaluate(address => {
        // Mock wallet validation
        return (
          typeof address === 'string' &&
          address.startsWith('0x') &&
          address.length === 42
        );
      }, wallet.address);
      return isValid;
    } catch {
      return false;
    }
  }

  // Seeding methods
  private async seedUser(page: Page, user: TestUser): Promise<void> {
    await page.evaluate(userData => {
      // Mock API call to seed user
      window.localStorage.setItem(
        `test_user_${userData.id}`,
        JSON.stringify(userData)
      );
    }, user);
  }

  private async seedWallet(page: Page, wallet: TestWallet): Promise<void> {
    await page.evaluate(walletData => {
      window.localStorage.setItem(
        `test_wallet_${walletData.id}`,
        JSON.stringify(walletData)
      );
    }, wallet);
  }

  private async seedWeatherData(
    page: Page,
    weatherData: WeatherData
  ): Promise<void> {
    await page.evaluate(data => {
      window.localStorage.setItem(
        `test_weather_${data.id}`,
        JSON.stringify(data)
      );
    }, weatherData);
  }

  private async seedFinancialData(
    page: Page,
    financialData: FinancialData
  ): Promise<void> {
    await page.evaluate(data => {
      window.localStorage.setItem(
        `test_financial_${data.id}`,
        JSON.stringify(data)
      );
    }, financialData);
  }

  private async seedIoTDevice(page: Page, device: IoTDevice): Promise<void> {
    await page.evaluate(deviceData => {
      window.localStorage.setItem(
        `test_device_${deviceData.id}`,
        JSON.stringify(deviceData)
      );
    }, device);
  }

  // Recovery and rollback methods
  async createDataSnapshot(testId: string): Promise<string> {
    const data = this.dataStore.get(testId);
    if (!data) throw new Error(`Test data not found: ${testId}`);

    const snapshotId = `snapshot_${testId}_${Date.now()}`;
    this.dataStore.set(snapshotId, JSON.parse(JSON.stringify(data)));
    return snapshotId;
  }

  async restoreFromSnapshot(snapshotId: string, page: Page): Promise<void> {
    const data = this.dataStore.get(snapshotId);
    if (!data) throw new Error(`Snapshot not found: ${snapshotId}`);

    await this.seedData(page, data);
  }

  // Add cleanup task
  addCleanupTask(task: () => Promise<void>): void {
    this.cleanupTasks.push(task);
  }
}

export default TestDataManager;
