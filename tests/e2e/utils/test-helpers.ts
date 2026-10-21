import { Page, expect } from '@playwright/test';
import { faker } from '@faker-js/faker';

// API Helper Functions
export class APIHelper {
  constructor(
    private baseURL: string = process.env.MAIN_API_URL ||
      'http://localhost:4001'
  ) {}

  async healthCheck(): Promise<boolean> {
    try {
      const response = await fetch(`${this.baseURL}/health`);
      return response.ok;
    } catch {
      return false;
    }
  }

  async authenticate(credentials: {
    username: string;
    password: string;
  }): Promise<string> {
    const response = await fetch(`${this.baseURL}/auth/login`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(credentials),
    });

    if (!response.ok) {
      throw new Error('Authentication failed');
    }

    const data = await response.json();
    return data.token;
  }

  // Enhanced API methods for test support
  async submitWeatherData(
    payload: any,
    apiKey: string = 'test-api-key'
  ): Promise<any> {
    const response = await fetch(
      `${process.env.IOT_SENSOR_NETWORKS_URL || 'http://localhost:3002'}/data/submit`,
      {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          'X-API-Key': apiKey,
        },
        body: JSON.stringify(payload),
      }
    );

    if (!response.ok) {
      throw new Error(`Weather data submission failed: ${response.statusText}`);
    }

    return await response.json();
  }

  async checkBridgeStatus(
    txHash: string
  ): Promise<{ status: string; destinationTxHash?: string }> {
    const response = await fetch(
      `${process.env.CROSS_CHAIN_URL || 'http://localhost:3000'}/bridge/status/${txHash}`
    );

    if (!response.ok) {
      return { status: 'unknown' };
    }

    return await response.json();
  }

  async getRewardBalance(userId: string, token?: string): Promise<number> {
    const response = await fetch(
      `${this.baseURL}/api/rewards/balance/${userId}${token ? `?token=${token}` : ''}`
    );

    if (!response.ok) {
      return 0;
    }

    const data = await response.json();
    return data.balance || 0;
  }

  async getDeviceSubmissions(deviceId: string): Promise<any[]> {
    const response = await fetch(
      `${process.env.IOT_SENSOR_NETWORKS_URL || 'http://localhost:3002'}/devices/${deviceId}/submissions`
    );

    if (!response.ok) {
      return [];
    }

    const data = await response.json();
    return data.submissions || [];
  }

  // Test cleanup methods
  async getTestDevices(): Promise<any[]> {
    const response = await fetch(
      `${process.env.IOT_SENSOR_NETWORKS_URL || 'http://localhost:3002'}/devices?test=true`
    );

    if (!response.ok) {
      return [];
    }

    const data = await response.json();
    return data.devices || [];
  }

  async deleteDevice(deviceId: string): Promise<boolean> {
    const response = await fetch(
      `${process.env.IOT_SENSOR_NETWORKS_URL || 'http://localhost:3002'}/devices/${deviceId}`,
      {
        method: 'DELETE',
      }
    );

    return response.ok;
  }

  async getTestSubmissions(): Promise<any[]> {
    const response = await fetch(
      `${process.env.IOT_SENSOR_NETWORKS_URL || 'http://localhost:3002'}/submissions?test=true`
    );

    if (!response.ok) {
      return [];
    }

    const data = await response.json();
    return data.submissions || [];
  }

  async deleteSubmission(submissionId: string): Promise<boolean> {
    const response = await fetch(
      `${process.env.IOT_SENSOR_NETWORKS_URL || 'http://localhost:3002'}/submissions/${submissionId}`,
      {
        method: 'DELETE',
      }
    );

    return response.ok;
  }

  async getTestUsers(): Promise<any[]> {
    const response = await fetch(`${this.baseURL}/api/users?test=true`);

    if (!response.ok) {
      return [];
    }

    const data = await response.json();
    return data.users || [];
  }

  async deleteUser(userId: string): Promise<boolean> {
    const response = await fetch(`${this.baseURL}/api/users/${userId}`, {
      method: 'DELETE',
    });

    return response.ok;
  }

  async waitForRewardProcessing(
    submissionId: string,
    timeoutMs: number = 30000
  ): Promise<boolean> {
    const startTime = Date.now();

    while (Date.now() - startTime < timeoutMs) {
      const response = await fetch(
        `${this.baseURL}/api/rewards/processing/${submissionId}`
      );

      if (response.ok) {
        const data = await response.json();
        if (data.status === 'completed') {
          return true;
        }
      }

      await new Promise(resolve => setTimeout(resolve, 1000));
    }

    return false;
  }
}

// Wallet Test Utilities
export class WalletTestUtils {
  static async connectWallet(page: Page) {
    // Click connect wallet button
    await page.click('[data-testid="connect-wallet-button"]');

    // Wait for wallet connection modal
    await page.waitForSelector('[data-testid="wallet-connection-modal"]');

    // Select MetaMask or test wallet
    await page.click('[data-testid="metamask-option"]');

    // Wait for connection confirmation
    await expect(
      page.locator('[data-testid="wallet-connected"]')
    ).toBeVisible();
  }

  static generateTestWalletData() {
    return {
      address: `0x${faker.string.hexadecimal({ length: 40, prefix: '' })}`,
      privateKey: `0x${faker.string.hexadecimal({ length: 64, prefix: '' })}`,
      mnemonic: faker.string
        .sample(12)
        .split(' ')
        .map(() => faker.word.sample())
        .join(' '),
    };
  }
}

// Weather Data Test Utilities
export class WeatherTestUtils {
  static generateWeatherData() {
    return {
      temperature: faker.number.float({ min: -30, max: 50, fractionDigits: 1 }),
      humidity: faker.number.int({ min: 0, max: 100 }),
      pressure: faker.number.float({ min: 950, max: 1050, fractionDigits: 1 }),
      windSpeed: faker.number.float({ min: 0, max: 200, fractionDigits: 1 }),
      timestamp: new Date().toISOString(),
      location: {
        lat: faker.location.latitude(),
        lon: faker.location.longitude(),
        name: faker.location.city(),
      },
    };
  }
}

// NFT Test Utilities
export class NFTTestUtils {
  static generateNFTMetadata() {
    return {
      name: faker.commerce.productName(),
      description: faker.commerce.productDescription(),
      image: `https://picsum.photos/400/400?random=${faker.number.int(1000)}`,
      attributes: [
        {
          trait_type: 'Rarity',
          value: faker.helpers.arrayElement([
            'Common',
            'Rare',
            'Epic',
            'Legendary',
          ]),
        },
        { trait_type: 'Power', value: faker.number.int({ min: 1, max: 100 }) },
      ],
    };
  }

  static async mintTestNFT(page: Page, metadata: any) {
    await page.click('[data-testid="mint-nft-button"]');
    await page.fill('[data-testid="nft-name-input"]', metadata.name);
    await page.fill(
      '[data-testid="nft-description-input"]',
      metadata.description
    );
    await page.fill('[data-testid="nft-image-url-input"]', metadata.image);
    await page.click('[data-testid="confirm-mint-button"]');

    // Wait for mint confirmation
    await expect(
      page.locator('[data-testid="mint-success-message"]')
    ).toBeVisible();
  }
}

// DeFi Test Utilities
export class DeFiTestUtils {
  static async addLiquidity(
    page: Page,
    tokenA: string,
    tokenB: string,
    amountA: number,
    amountB: number
  ) {
    await page.click('[data-testid="add-liquidity-button"]');
    await page.selectOption('[data-testid="token-a-select"]', tokenA);
    await page.selectOption('[data-testid="token-b-select"]', tokenB);
    await page.fill('[data-testid="amount-a-input"]', amountA.toString());
    await page.fill('[data-testid="amount-b-input"]', amountB.toString());
    await page.click('[data-testid="confirm-add-liquidity"]');

    // Wait for transaction confirmation
    await expect(
      page.locator('[data-testid="liquidity-added-confirmation"]')
    ).toBeVisible();
  }

  static async stake(page: Page, amount: number, duration: number) {
    await page.click('[data-testid="stake-button"]');
    await page.fill('[data-testid="stake-amount-input"]', amount.toString());
    await page.selectOption(
      '[data-testid="stake-duration-select"]',
      duration.toString()
    );
    await page.click('[data-testid="confirm-stake"]');

    // Wait for staking confirmation
    await expect(
      page.locator('[data-testid="stake-confirmation"]')
    ).toBeVisible();
  }
}

// Navigation Utilities
export class NavigationUtils {
  static async navigateToWallet(page: Page) {
    await page.click('[data-testid="nav-wallet"]');
    await expect(page).toHaveURL(/.*\/wallet/);
  }

  static async navigateToNFTMarketplace(page: Page) {
    await page.click('[data-testid="nav-nft-marketplace"]');
    await expect(page).toHaveURL(/.*\/nft-marketplace/);
  }

  static async navigateToDeFi(page: Page) {
    await page.click('[data-testid="nav-defi"]');
    await expect(page).toHaveURL(/.*\/defi/);
  }

  static async navigateToGovernance(page: Page) {
    await page.click('[data-testid="nav-governance"]');
    await expect(page).toHaveURL(/.*\/governance/);
  }

  static async navigateToCarbonCredits(page: Page) {
    await page.click('[data-testid="nav-carbon-credits"]');
    await expect(page).toHaveURL(/.*\/carbon-credits/);
  }
}

// Wait Utilities
export class WaitUtils {
  static async waitForTransaction(page: Page) {
    await page.waitForSelector('[data-testid="transaction-pending"]');
    await page.waitForSelector('[data-testid="transaction-confirmed"]', {
      timeout: 60000,
    });
  }

  static async waitForPageLoad(page: Page) {
    await page.waitForLoadState('domcontentloaded');
    await page.waitForSelector('[data-testid="page-loaded"]', {
      timeout: 30000,
    });
  }

  static async waitForAPICall(page: Page, apiEndpoint: string) {
    await page.waitForResponse(
      response =>
        response.url().includes(apiEndpoint) && response.status() === 200
    );
  }
}

// Assertion Utilities
export class AssertionUtils {
  static async assertWalletConnected(page: Page) {
    await expect(
      page.locator('[data-testid="wallet-connected"]')
    ).toBeVisible();
    await expect(page.locator('[data-testid="wallet-address"]')).toBeVisible();
  }

  static async assertTransactionSuccess(page: Page) {
    await expect(
      page.locator('[data-testid="transaction-success"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="transaction-hash"]')
    ).toBeVisible();
  }

  static async assertErrorMessage(page: Page, expectedMessage: string) {
    const errorElement = page.locator('[data-testid="error-message"]');
    await expect(errorElement).toBeVisible();
    await expect(errorElement).toContainText(expectedMessage);
  }
}
