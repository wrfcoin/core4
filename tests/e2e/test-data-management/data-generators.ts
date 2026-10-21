// Realistic Test Data Generators for WRFCoin Platform
import crypto from 'crypto';

export interface WeatherDataGeneratorConfig {
  location: { lat: number; lon: number; name: string };
  timeRange: { start: Date; end: Date };
  interval: number; // seconds between readings
  seasonalPattern: boolean;
  qualityVariation: boolean;
  anomalies: boolean;
}

export interface CryptoWalletConfig {
  chainIds: number[];
  tokenTypes: string[];
  balanceRange: { min: number; max: number };
  transactionHistory: boolean;
}

export interface TransactionHistoryConfig {
  count: number;
  timeRange: { start: Date; end: Date };
  types: string[];
  successRate: number;
}

export class WeatherDataGenerator {
  private config: WeatherDataGeneratorConfig;
  private random: () => number;

  constructor(config: WeatherDataGeneratorConfig, seed?: string) {
    this.config = config;
    // Use seeded random for reproducible data
    if (seed) {
      const seedNum = this.hashString(seed);
      this.random = this.seededRandom(seedNum);
    } else {
      this.random = Math.random;
    }
  }

  generate(): any[] {
    const data = [];
    const { start, end } = this.config.timeRange;
    const duration = end.getTime() - start.getTime();
    const readings = Math.floor(duration / (this.config.interval * 1000));

    for (let i = 0; i < readings; i++) {
      const timestamp = new Date(
        start.getTime() + i * this.config.interval * 1000
      );
      const reading = this.generateReading(timestamp, i);
      data.push(reading);
    }

    return data;
  }

  private generateReading(timestamp: Date, index: number): any {
    const dayOfYear = this.getDayOfYear(timestamp);
    const hourOfDay = timestamp.getHours();

    // Base temperature with seasonal variation
    let temperature = 15; // Base temperature
    if (this.config.seasonalPattern) {
      temperature +=
        10 * Math.sin((dayOfYear / 365) * 2 * Math.PI - Math.PI / 2); // Seasonal variation
      temperature += 8 * Math.sin((hourOfDay / 24) * 2 * Math.PI - Math.PI / 2); // Diurnal variation
    }
    temperature += (this.random() - 0.5) * 4; // Random variation

    // Humidity with inverse correlation to temperature
    let humidity = 70 - (temperature - 15) * 1.5;
    humidity += (this.random() - 0.5) * 20;
    humidity = Math.max(10, Math.min(100, humidity));

    // Pressure with weather system simulation
    let pressure = 1013.25;
    pressure += Math.sin((index / 48) * 2 * Math.PI) * 20; // Multi-day weather cycles
    pressure += (this.random() - 0.5) * 10;

    // Wind speed with daily patterns
    let windSpeed = 5 + Math.sin((hourOfDay / 24) * 2 * Math.PI) * 3;
    windSpeed += this.random() * 10;
    windSpeed = Math.max(0, windSpeed);

    // Wind direction - somewhat consistent with variation
    let windDirection =
      (180 + Math.sin((index / 100) * 2 * Math.PI) * 60) % 360;
    windDirection += (this.random() - 0.5) * 45;
    windDirection = (windDirection + 360) % 360;

    // Rainfall (occasional)
    let rainfall = 0;
    if (this.random() < 0.1) {
      // 10% chance of rain
      rainfall = this.random() * 15;
    }

    // Quality score with variation
    let qualityScore = 0.9;
    if (this.config.qualityVariation) {
      qualityScore = 0.7 + this.random() * 0.3;
    }

    // Add anomalies
    if (this.config.anomalies && this.random() < 0.02) {
      // 2% chance
      temperature += (this.random() - 0.5) * 20;
      qualityScore *= 0.5;
    }

    return {
      id: `weather_${timestamp.getTime()}_${crypto.randomBytes(4).toString('hex')}`,
      deviceId: `device_${this.hashString(this.config.location.name) % 100}`,
      timestamp: timestamp.toISOString(),
      location: this.config.location,
      measurements: {
        temperature: Math.round(temperature * 100) / 100,
        humidity: Math.round(humidity * 100) / 100,
        pressure: Math.round(pressure * 100) / 100,
        windSpeed: Math.round(windSpeed * 100) / 100,
        windDirection: Math.round(windDirection),
        rainfall: Math.round(rainfall * 100) / 100,
        solarRadiation: Math.max(
          0,
          1000 * Math.sin(((hourOfDay - 6) / 12) * Math.PI)
        ),
      },
      qualityScore: Math.round(qualityScore * 1000) / 1000,
      verified: qualityScore > 0.8,
    };
  }

  private getDayOfYear(date: Date): number {
    const start = new Date(date.getFullYear(), 0, 0);
    const diff = date.getTime() - start.getTime();
    return Math.floor(diff / (1000 * 60 * 60 * 24));
  }

  private hashString(str: string): number {
    let hash = 0;
    for (let i = 0; i < str.length; i++) {
      const char = str.charCodeAt(i);
      hash = (hash << 5) - hash + char;
      hash = hash & hash; // Convert to 32-bit integer
    }
    return Math.abs(hash);
  }

  private seededRandom(seed: number): () => number {
    return function () {
      seed = (seed * 9301 + 49297) % 233280;
      return seed / 233280;
    };
  }
}

export class CryptoWalletGenerator {
  private config: CryptoWalletConfig;

  constructor(config: CryptoWalletConfig) {
    this.config = config;
  }

  generate(): any[] {
    const wallets = [];

    for (const chainId of this.config.chainIds) {
      const wallet = this.generateWallet(chainId);
      wallets.push(wallet);
    }

    return wallets;
  }

  private generateWallet(chainId: number): any {
    const privateKey = '0x' + crypto.randomBytes(32).toString('hex');
    const address = '0x' + crypto.randomBytes(20).toString('hex');
    const mnemonic = this.generateMnemonic();

    const balances: Record<string, string> = {};
    for (const token of this.config.tokenTypes) {
      const balance =
        this.config.balanceRange.min +
        Math.random() *
          (this.config.balanceRange.max - this.config.balanceRange.min);
      balances[token] = balance.toFixed(6);
    }

    const wallet = {
      id: `wallet_${chainId}_${crypto.randomBytes(4).toString('hex')}`,
      address,
      privateKey,
      mnemonic,
      chainId,
      networkName: this.getNetworkName(chainId),
      balances,
      transactions: [],
      type: this.getRandomWalletType(),
    };

    if (this.config.transactionHistory) {
      wallet.transactions = this.generateTransactionHistory(address);
    }

    return wallet;
  }

  private generateTransactionHistory(address: string): any[] {
    const transactions = [];
    const count = 10 + Math.floor(Math.random() * 40); // 10-50 transactions

    for (let i = 0; i < count; i++) {
      const timestamp = new Date(
        Date.now() - Math.random() * 30 * 24 * 60 * 60 * 1000
      );

      transactions.push({
        id: `tx_${i}_${crypto.randomBytes(4).toString('hex')}`,
        hash: '0x' + crypto.randomBytes(32).toString('hex'),
        from:
          Math.random() > 0.5
            ? address
            : '0x' + crypto.randomBytes(20).toString('hex'),
        to:
          Math.random() > 0.5
            ? address
            : '0x' + crypto.randomBytes(20).toString('hex'),
        value: (Math.random() * 10).toFixed(6),
        gasUsed: (21000 + Math.random() * 50000).toString(),
        gasPrice: (20 + Math.random() * 100).toFixed(0),
        timestamp: timestamp.toISOString(),
        status: Math.random() > 0.05 ? 'confirmed' : 'failed', // 95% success rate
        type: this.getRandomTransactionType(),
      });
    }

    return transactions.sort(
      (a, b) =>
        new Date(b.timestamp).getTime() - new Date(a.timestamp).getTime()
    );
  }

  private generateMnemonic(): string {
    const words = [
      'abandon',
      'ability',
      'able',
      'about',
      'above',
      'absent',
      'absorb',
      'abstract',
      'absurd',
      'abuse',
      'access',
      'accident',
      'account',
      'accuse',
      'achieve',
      'acid',
      'acoustic',
      'acquire',
      'across',
      'act',
      'action',
      'actor',
      'actress',
      'actual',
    ];

    const mnemonic = [];
    for (let i = 0; i < 12; i++) {
      mnemonic.push(words[Math.floor(Math.random() * words.length)]);
    }
    return mnemonic.join(' ');
  }

  private getNetworkName(chainId: number): string {
    const networks: Record<number, string> = {
      1: 'Ethereum Mainnet',
      137: 'Polygon',
      56: 'BSC',
      43114: 'Avalanche',
      31337: 'Localhost',
    };
    return networks[chainId] || 'Unknown Network';
  }

  private getRandomWalletType(): string {
    const types = ['metamask', 'walletconnect', 'platform', 'hardware'];
    return types[Math.floor(Math.random() * types.length)];
  }

  private getRandomTransactionType(): string {
    const types = [
      'transfer',
      'swap',
      'liquidity',
      'stake',
      'harvest',
      'bridge',
      'mint',
      'approve',
    ];
    return types[Math.floor(Math.random() * types.length)];
  }
}

export class FinancialDataGenerator {
  generate(userId: string, walletAddress: string): any {
    return {
      id: `financial_${userId}`,
      userId,
      walletAddress,
      positions: {
        liquidityPools: this.generateLiquidityPositions(),
        stakingPositions: this.generateStakingPositions(),
        yieldFarms: this.generateYieldFarmPositions(),
      },
      transactionHistory: this.generateFinancialTransactions(),
      totalValue: 5000 + Math.random() * 45000,
      pnl: -2000 + Math.random() * 4000, // -2000 to +2000
    };
  }

  private generateLiquidityPositions(): any[] {
    const pools = [
      { token0: 'WRF', token1: 'ETH' },
      { token0: 'WRF', token1: 'CARBON' },
      { token0: 'ETH', token1: 'USDC' },
      { token0: 'CARBON', token1: 'USDC' },
    ];

    return pools.slice(0, 1 + Math.floor(Math.random() * 3)).map(pool => ({
      poolId: `${pool.token0}-${pool.token1}`,
      token0: pool.token0,
      token1: pool.token1,
      amount0: (100 + Math.random() * 900).toFixed(2),
      amount1: (0.1 + Math.random() * 9.9).toFixed(4),
      lpTokens: (50 + Math.random() * 450).toFixed(6),
      apy: 0.05 + Math.random() * 0.25,
      impermanentLoss: -0.05 + Math.random() * 0.1,
    }));
  }

  private generateStakingPositions(): any[] {
    const stakingPools = ['WRF-STAKE', 'CARBON-STAKE', 'LP-WRF-ETH-STAKE'];

    return stakingPools
      .slice(0, 1 + Math.floor(Math.random() * 2))
      .map(pool => ({
        stakingPool: pool,
        stakedAmount: (200 + Math.random() * 1800).toFixed(2),
        rewardsEarned: (5 + Math.random() * 95).toFixed(4),
        apy: 0.08 + Math.random() * 0.22,
        lockupPeriod: [7, 14, 30, 90][Math.floor(Math.random() * 4)],
        maturityDate: new Date(
          Date.now() + (7 + Math.random() * 83) * 24 * 60 * 60 * 1000
        ).toISOString(),
      }));
  }

  private generateYieldFarmPositions(): any[] {
    const farms = ['WEATHER-FARM', 'CARBON-FARM', 'COMPOUND-FARM'];

    return farms.slice(0, Math.floor(Math.random() * 3)).map(farm => ({
      farmId: farm,
      stakedTokens: (300 + Math.random() * 1200).toFixed(2),
      rewardTokens: 'WRF',
      apy: 0.12 + Math.random() * 0.38,
      harvestedRewards: (10 + Math.random() * 90).toFixed(4),
      autoCompound: Math.random() > 0.5,
    }));
  }

  private generateFinancialTransactions(): any[] {
    const transactions = [];
    const count = 5 + Math.floor(Math.random() * 15);

    const types = [
      'swap',
      'liquidity-add',
      'liquidity-remove',
      'stake',
      'unstake',
      'harvest',
      'bridge',
    ];

    for (let i = 0; i < count; i++) {
      const timestamp = new Date(
        Date.now() - Math.random() * 30 * 24 * 60 * 60 * 1000
      );

      transactions.push({
        id: `fin_tx_${i}_${crypto.randomBytes(4).toString('hex')}`,
        type: types[Math.floor(Math.random() * types.length)],
        amount: (0.1 + Math.random() * 999.9).toFixed(4),
        token: ['WRF', 'ETH', 'CARBON', 'USDC'][Math.floor(Math.random() * 4)],
        timestamp: timestamp.toISOString(),
        gasUsed: (21000 + Math.random() * 200000).toString(),
        gasCost: (0.001 + Math.random() * 0.099).toFixed(6),
        status: Math.random() > 0.02 ? 'confirmed' : 'failed',
        hash: '0x' + crypto.randomBytes(32).toString('hex'),
      });
    }

    return transactions.sort(
      (a, b) =>
        new Date(b.timestamp).getTime() - new Date(a.timestamp).getTime()
    );
  }
}

export class IoTDeviceGenerator {
  generate(count: number): any[] {
    const devices = [];
    const manufacturers = [
      'Acurite',
      'WeatherFlow',
      'Ambient Weather',
      'Davis Instruments',
      'Oregon Scientific',
      'La Crosse Technology',
    ];
    const models = [
      'Atlas',
      'Tempest',
      'WS-2902',
      'Vantage Pro2',
      'BAR88',
      'WS-1550',
    ];
    const locations = [
      { name: 'New York', lat: 40.7128, lon: -74.006 },
      { name: 'Los Angeles', lat: 34.0522, lon: -118.2437 },
      { name: 'Chicago', lat: 41.8781, lon: -87.6298 },
      { name: 'Houston', lat: 29.7604, lon: -95.3698 },
      { name: 'Phoenix', lat: 33.4484, lon: -112.074 },
      { name: 'Philadelphia', lat: 39.9526, lon: -75.1652 },
    ];

    for (let i = 0; i < count; i++) {
      const manufacturer =
        manufacturers[Math.floor(Math.random() * manufacturers.length)];
      const model = models[Math.floor(Math.random() * models.length)];
      const location = locations[Math.floor(Math.random() * locations.length)];

      // Add some random offset to location for uniqueness
      const deviceLocation = {
        name: `${location.name} Station ${i + 1}`,
        lat: location.lat + (Math.random() - 0.5) * 0.1,
        lon: location.lon + (Math.random() - 0.5) * 0.1,
      };

      devices.push({
        id: `device_${i.toString().padStart(3, '0')}`,
        serialNumber: `${manufacturer.toUpperCase()}-${model.replace(/[^A-Z0-9]/g, '')}-${crypto.randomBytes(4).toString('hex').toUpperCase()}`,
        type: 'weather_station',
        model,
        manufacturer,
        firmware: `${Math.floor(Math.random() * 3) + 1}.${Math.floor(Math.random() * 10)}.${Math.floor(Math.random() * 10)}`,
        location: deviceLocation,
        status: this.getRandomStatus(),
        lastDataTransmission: new Date(
          Date.now() - Math.random() * 3600000
        ).toISOString(),
        configuration: {
          dataInterval: [60, 300, 600, 900][Math.floor(Math.random() * 4)], // 1min, 5min, 10min, 15min
          qualityThreshold: 0.7 + Math.random() * 0.2,
          autoCalibration: Math.random() > 0.3,
          batteryLevel: Math.floor(Math.random() * 100),
          signalStrength: -30 - Math.random() * 40, // -30 to -70 dBm
          uptime: Math.floor(Math.random() * 365 * 24 * 60), // minutes
        },
        owner: `user_${Math.floor(Math.random() * 50)}`,
        installationDate: new Date(
          Date.now() - Math.random() * 365 * 24 * 60 * 60 * 1000
        ).toISOString(),
        maintenanceSchedule: {
          lastMaintenance: new Date(
            Date.now() - Math.random() * 90 * 24 * 60 * 60 * 1000
          ).toISOString(),
          nextMaintenance: new Date(
            Date.now() + (30 + Math.random() * 60) * 24 * 60 * 60 * 1000
          ).toISOString(),
        },
        dataStats: {
          totalReadings: Math.floor(Math.random() * 100000),
          averageQuality: 0.8 + Math.random() * 0.2,
          uptimePercentage: 85 + Math.random() * 15,
          lastCalibration: new Date(
            Date.now() - Math.random() * 180 * 24 * 60 * 60 * 1000
          ).toISOString(),
        },
      });
    }

    return devices;
  }

  private getRandomStatus(): string {
    const statuses = [
      { status: 'active', weight: 0.7 },
      { status: 'inactive', weight: 0.15 },
      { status: 'maintenance', weight: 0.1 },
      { status: 'error', weight: 0.05 },
    ];

    const random = Math.random();
    let cumulative = 0;

    for (const { status, weight } of statuses) {
      cumulative += weight;
      if (random <= cumulative) {
        return status;
      }
    }

    return 'active';
  }
}

export class CrossChainDataGenerator {
  generate(): any {
    return {
      id: 'crosschain_test_data',
      bridges: this.generateBridgeTransactions(),
      crossChainAssets: this.generateCrossChainAssets(),
      supportedChains: this.generateSupportedChains(),
    };
  }

  private generateBridgeTransactions(): any[] {
    const transactions = [];
    const count = 5 + Math.floor(Math.random() * 15);
    const chains = ['ethereum', 'polygon', 'bsc', 'avalanche'];
    const tokens = ['WRF', 'CARBON', 'ETH', 'USDC'];

    for (let i = 0; i < count; i++) {
      const sourceChain = chains[Math.floor(Math.random() * chains.length)];
      let destinationChain = chains[Math.floor(Math.random() * chains.length)];
      while (destinationChain === sourceChain) {
        destinationChain = chains[Math.floor(Math.random() * chains.length)];
      }

      const token = tokens[Math.floor(Math.random() * tokens.length)];
      const timestamp = new Date(
        Date.now() - Math.random() * 30 * 24 * 60 * 60 * 1000
      );

      transactions.push({
        id: `bridge_${i}_${crypto.randomBytes(4).toString('hex')}`,
        sourceChain,
        destinationChain,
        sourceToken: token,
        destinationToken: token,
        amount: (1 + Math.random() * 999).toFixed(4),
        bridgeProvider: 'WRFCoin Bridge',
        status: this.getRandomBridgeStatus(),
        fees: (0.1 + Math.random() * 9.9).toFixed(4),
        estimatedTime: `${5 + Math.floor(Math.random() * 25)} minutes`,
        actualTime: `${3 + Math.floor(Math.random() * 30)} minutes`,
        timestamp: timestamp.toISOString(),
        sourceTransactionHash: '0x' + crypto.randomBytes(32).toString('hex'),
        destinationTransactionHash:
          Math.random() > 0.2
            ? '0x' + crypto.randomBytes(32).toString('hex')
            : null,
        user: `0x${crypto.randomBytes(20).toString('hex')}`,
      });
    }

    return transactions.sort(
      (a, b) =>
        new Date(b.timestamp).getTime() - new Date(a.timestamp).getTime()
    );
  }

  private generateCrossChainAssets(): any[] {
    return [
      {
        id: 'wrf_multichain',
        symbol: 'WRF',
        name: 'WRFCoin Token',
        supportedChains: ['ethereum', 'polygon', 'bsc', 'avalanche'],
        totalSupply: '1000000000',
        bridgeable: true,
        contractAddresses: {
          ethereum: '0x' + crypto.randomBytes(20).toString('hex'),
          polygon: '0x' + crypto.randomBytes(20).toString('hex'),
          bsc: '0x' + crypto.randomBytes(20).toString('hex'),
          avalanche: '0x' + crypto.randomBytes(20).toString('hex'),
        },
      },
      {
        id: 'carbon_multichain',
        symbol: 'CARBON',
        name: 'Carbon Credits Token',
        supportedChains: ['ethereum', 'polygon'],
        totalSupply: '500000000',
        bridgeable: true,
        contractAddresses: {
          ethereum: '0x' + crypto.randomBytes(20).toString('hex'),
          polygon: '0x' + crypto.randomBytes(20).toString('hex'),
        },
      },
    ];
  }

  private generateSupportedChains(): any[] {
    return [
      {
        id: 'ethereum',
        name: 'Ethereum',
        chainId: 1,
        rpcUrl: 'https://mainnet.infura.io/v3/test',
        blockExplorer: 'https://etherscan.io',
        nativeCurrency: { name: 'Ethereum', symbol: 'ETH', decimals: 18 },
        averageFees: { slow: '20', standard: '35', fast: '50' },
        blockTime: 15,
        bridgeCapacity: '10000000',
      },
      {
        id: 'polygon',
        name: 'Polygon',
        chainId: 137,
        rpcUrl: 'https://polygon-rpc.com',
        blockExplorer: 'https://polygonscan.com',
        nativeCurrency: { name: 'MATIC', symbol: 'MATIC', decimals: 18 },
        averageFees: { slow: '1', standard: '2', fast: '5' },
        blockTime: 2,
        bridgeCapacity: '5000000',
      },
      {
        id: 'bsc',
        name: 'Binance Smart Chain',
        chainId: 56,
        rpcUrl: 'https://bsc-dataseed.binance.org',
        blockExplorer: 'https://bscscan.com',
        nativeCurrency: { name: 'BNB', symbol: 'BNB', decimals: 18 },
        averageFees: { slow: '3', standard: '5', fast: '10' },
        blockTime: 3,
        bridgeCapacity: '3000000',
      },
      {
        id: 'avalanche',
        name: 'Avalanche',
        chainId: 43114,
        rpcUrl: 'https://api.avax.network/ext/bc/C/rpc',
        blockExplorer: 'https://snowtrace.io',
        nativeCurrency: { name: 'AVAX', symbol: 'AVAX', decimals: 18 },
        averageFees: { slow: '5', standard: '10', fast: '20' },
        blockTime: 2,
        bridgeCapacity: '2000000',
      },
    ];
  }

  private getRandomBridgeStatus(): string {
    const statuses = [
      { status: 'completed', weight: 0.8 },
      { status: 'pending', weight: 0.15 },
      { status: 'failed', weight: 0.05 },
    ];

    const random = Math.random();
    let cumulative = 0;

    for (const { status, weight } of statuses) {
      cumulative += weight;
      if (random <= cumulative) {
        return status;
      }
    }

    return 'completed';
  }
}

export default {
  WeatherDataGenerator,
  CryptoWalletGenerator,
  FinancialDataGenerator,
  IoTDeviceGenerator,
  CrossChainDataGenerator,
};
