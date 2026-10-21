import { Page, Locator, expect } from '@playwright/test';
import { BasePage } from '../common/base-page';

export class WalletPage extends BasePage {
  // Wallet-specific locators
  readonly connectWalletButton: Locator;
  readonly walletConnectionModal: Locator;
  readonly metamaskOption: Locator;
  readonly walletConnectedIndicator: Locator;
  readonly walletAddress: Locator;
  readonly walletBalance: Locator;
  readonly sendButton: Locator;
  readonly receiveButton: Locator;
  readonly transactionHistory: Locator;
  readonly addTokenButton: Locator;
  readonly tokenList: Locator;
  readonly disconnectWalletButton: Locator;

  // Transaction form
  readonly sendModal: Locator;
  readonly recipientAddressInput: Locator;
  readonly sendAmountInput: Locator;
  readonly tokenSelectDropdown: Locator;
  readonly confirmSendButton: Locator;
  readonly cancelSendButton: Locator;

  // Receive modal
  readonly receiveModal: Locator;
  readonly qrCodeImage: Locator;
  readonly copyAddressButton: Locator;

  constructor(page: Page) {
    super(page);

    // Initialize wallet-specific locators
    this.connectWalletButton = page.locator(
      '[data-testid="connect-wallet-button"]'
    );
    this.walletConnectionModal = page.locator(
      '[data-testid="wallet-connection-modal"]'
    );
    this.metamaskOption = page.locator('[data-testid="metamask-option"]');
    this.walletConnectedIndicator = page.locator(
      '[data-testid="wallet-connected"]'
    );
    this.walletAddress = page.locator('[data-testid="wallet-address"]');
    this.walletBalance = page.locator('[data-testid="wallet-balance"]');
    this.sendButton = page.locator('[data-testid="send-button"]');
    this.receiveButton = page.locator('[data-testid="receive-button"]');
    this.transactionHistory = page.locator(
      '[data-testid="transaction-history"]'
    );
    this.addTokenButton = page.locator('[data-testid="add-token-button"]');
    this.tokenList = page.locator('[data-testid="token-list"]');
    this.disconnectWalletButton = page.locator(
      '[data-testid="disconnect-wallet-button"]'
    );

    // Transaction form locators
    this.sendModal = page.locator('[data-testid="send-modal"]');
    this.recipientAddressInput = page.locator(
      '[data-testid="recipient-address-input"]'
    );
    this.sendAmountInput = page.locator('[data-testid="send-amount-input"]');
    this.tokenSelectDropdown = page.locator(
      '[data-testid="token-select-dropdown"]'
    );
    this.confirmSendButton = page.locator(
      '[data-testid="confirm-send-button"]'
    );
    this.cancelSendButton = page.locator('[data-testid="cancel-send-button"]');

    // Receive modal locators
    this.receiveModal = page.locator('[data-testid="receive-modal"]');
    this.qrCodeImage = page.locator('[data-testid="qr-code-image"]');
    this.copyAddressButton = page.locator(
      '[data-testid="copy-address-button"]'
    );
  }

  // Wallet Actions
  async gotoWallet() {
    await this.goto('/wallet');
    await expect(this.page).toHaveURL(/.*\/wallet/);
  }

  async connectWallet() {
    await this.connectWalletButton.click();
    await expect(this.walletConnectionModal).toBeVisible();
    await this.metamaskOption.click();
    await expect(this.walletConnectedIndicator).toBeVisible();
  }

  async disconnectWallet() {
    await this.disconnectWalletButton.click();
    await expect(this.connectWalletButton).toBeVisible();
  }

  async getWalletAddress(): Promise<string> {
    return (await this.walletAddress.textContent()) || '';
  }

  async getWalletBalance(): Promise<string> {
    return (await this.walletBalance.textContent()) || '0';
  }

  // Send Transaction
  async sendTransaction(
    recipientAddress: string,
    amount: string,
    token: string = 'WRF'
  ) {
    await this.sendButton.click();
    await expect(this.sendModal).toBeVisible();

    await this.recipientAddressInput.fill(recipientAddress);
    await this.sendAmountInput.fill(amount);
    await this.tokenSelectDropdown.selectOption(token);

    await this.confirmSendButton.click();

    // Wait for transaction confirmation
    await this.waitForAPIResponse('transactions');
    await expect(this.successMessage).toBeVisible();
  }

  // Receive Transaction
  async openReceiveModal() {
    await this.receiveButton.click();
    await expect(this.receiveModal).toBeVisible();
    await expect(this.qrCodeImage).toBeVisible();
  }

  async copyWalletAddress() {
    await this.openReceiveModal();
    await this.copyAddressButton.click();
    await expect(this.successMessage).toContainText('Address copied');
  }

  // Token Management
  async addToken(tokenAddress: string, symbol: string, decimals: string) {
    await this.addTokenButton.click();

    const addTokenModal = this.page.locator('[data-testid="add-token-modal"]');
    await expect(addTokenModal).toBeVisible();

    await this.page
      .locator('[data-testid="token-address-input"]')
      .fill(tokenAddress);
    await this.page.locator('[data-testid="token-symbol-input"]').fill(symbol);
    await this.page
      .locator('[data-testid="token-decimals-input"]')
      .fill(decimals);

    await this.page.locator('[data-testid="confirm-add-token"]').click();
    await expect(this.successMessage).toContainText('Token added successfully');
  }

  // Transaction History
  async getTransactionHistory(): Promise<
    Array<{ hash: string; amount: string; status: string }>
  > {
    const transactions = [];
    const transactionRows = this.transactionHistory.locator(
      '[data-testid="transaction-row"]'
    );
    const count = await transactionRows.count();

    for (let i = 0; i < count; i++) {
      const row = transactionRows.nth(i);
      transactions.push({
        hash:
          (await row
            .locator('[data-testid="transaction-hash"]')
            .textContent()) || '',
        amount:
          (await row
            .locator('[data-testid="transaction-amount"]')
            .textContent()) || '',
        status:
          (await row
            .locator('[data-testid="transaction-status"]')
            .textContent()) || '',
      });
    }

    return transactions;
  }

  async waitForTransactionConfirmation(transactionHash: string) {
    const transactionRow = this.transactionHistory.locator(
      `[data-testid="transaction-row"][data-hash="${transactionHash}"]`
    );
    const statusLocator = transactionRow.locator(
      '[data-testid="transaction-status"]'
    );

    // Wait for status to change from "pending" to "confirmed"
    await expect(statusLocator).not.toContainText('pending', {
      timeout: 60000,
    });
    await expect(statusLocator).toContainText('confirmed');
  }

  // Assertions
  async assertWalletConnected() {
    await expect(this.walletConnectedIndicator).toBeVisible();
    await expect(this.walletAddress).toBeVisible();
    await expect(this.walletBalance).toBeVisible();
  }

  async assertWalletDisconnected() {
    await expect(this.connectWalletButton).toBeVisible();
    await expect(this.walletConnectedIndicator).not.toBeVisible();
  }

  async assertTransactionInHistory(transactionHash: string) {
    const transactionRow = this.transactionHistory.locator(
      `[data-testid="transaction-row"][data-hash="${transactionHash}"]`
    );
    await expect(transactionRow).toBeVisible();
  }
}
