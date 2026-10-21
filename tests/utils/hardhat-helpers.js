/**
 * Hardhat Test Helpers
 * Utilities for smart contract testing and blockchain interaction
 */

const { ethers } = require('hardhat');
const { expect } = require('chai');

class HardhatTestHelper {
  constructor() {
    this.deployedContracts = new Map();
    this.accounts = [];
    this.snapshots = [];
  }

  /**
   * Setup test environment
   */
  async setup() {
    // Get test accounts
    this.accounts = await ethers.getSigners();

    // Take initial snapshot
    const snapshot = await this.takeSnapshot();
    this.snapshots.push(snapshot);

    return {
      accounts: this.accounts,
      deployer: this.accounts[0],
      user1: this.accounts[1],
      user2: this.accounts[2],
      admin: this.accounts[3],
    };
  }

  /**
   * Deploy contract with caching
   */
  async deployContract(contractName, args = [], options = {}) {
    const cacheKey = `${contractName}-${JSON.stringify(args)}`;

    if (this.deployedContracts.has(cacheKey) && !options.fresh) {
      return this.deployedContracts.get(cacheKey);
    }

    try {
      const ContractFactory = await ethers.getContractFactory(contractName);
      const contract = await ContractFactory.deploy(...args);
      await contract.waitForDeployment();

      const deployedContract = {
        contract,
        address: await contract.getAddress(),
        factory: ContractFactory,
        deploymentTx: contract.deploymentTransaction(),
      };

      this.deployedContracts.set(cacheKey, deployedContract);

      console.log(`📋 Deployed ${contractName} at ${deployedContract.address}`);

      return deployedContract;
    } catch (error) {
      console.error(`Failed to deploy ${contractName}:`, error.message);
      throw error;
    }
  }

  /**
   * Deploy multiple contracts in sequence
   */
  async deployContracts(contractSpecs) {
    const deployed = {};

    for (const [name, spec] of Object.entries(contractSpecs)) {
      const { contractName, args = [], options = {} } = spec;
      deployed[name] = await this.deployContract(contractName, args, options);
    }

    return deployed;
  }

  /**
   * Take blockchain snapshot
   */
  async takeSnapshot() {
    return await ethers.provider.send('evm_snapshot', []);
  }

  /**
   * Restore blockchain snapshot
   */
  async restoreSnapshot(snapshotId) {
    await ethers.provider.send('evm_revert', [snapshotId]);
  }

  /**
   * Mine blocks
   */
  async mineBlocks(count) {
    for (let i = 0; i < count; i++) {
      await ethers.provider.send('evm_mine', []);
    }
  }

  /**
   * Increase blockchain time
   */
  async increaseTime(seconds) {
    await ethers.provider.send('evm_increaseTime', [seconds]);
    await ethers.provider.send('evm_mine', []);
  }

  /**
   * Set next block timestamp
   */
  async setNextBlockTimestamp(timestamp) {
    await ethers.provider.send('evm_setNextBlockTimestamp', [timestamp]);
  }

  /**
   * Get latest block
   */
  async getLatestBlock() {
    return await ethers.provider.getBlock('latest');
  }

  /**
   * Impersonate account
   */
  async impersonateAccount(address) {
    await ethers.provider.send('hardhat_impersonateAccount', [address]);
    return await ethers.getSigner(address);
  }

  /**
   * Stop impersonating account
   */
  async stopImpersonatingAccount(address) {
    await ethers.provider.send('hardhat_stopImpersonatingAccount', [address]);
  }

  /**
   * Set account balance
   */
  async setBalance(address, balance) {
    await ethers.provider.send('hardhat_setBalance', [
      address,
      ethers.toQuantity(balance),
    ]);
  }

  /**
   * Generate test tokens/NFTs
   */
  async generateTestAssets(count = 10) {
    const assets = [];

    for (let i = 0; i < count; i++) {
      assets.push({
        id: i + 1,
        name: `Test Asset ${i + 1}`,
        description: `Test asset for integration testing`,
        metadata: {
          type: 'test',
          value: Math.floor(Math.random() * 1000) + 1,
          rarity: ['common', 'uncommon', 'rare'][Math.floor(Math.random() * 3)],
        },
      });
    }

    return assets;
  }

  /**
   * Estimate gas for transaction
   */
  async estimateGas(contract, methodName, args = []) {
    try {
      const gasEstimate = await contract[methodName].estimateGas(...args);
      return {
        estimated: gasEstimate,
        withBuffer: (gasEstimate * 120n) / 100n, // 20% buffer
        formatted: ethers.formatUnits(gasEstimate, 'gwei'),
      };
    } catch (error) {
      console.error(`Gas estimation failed for ${methodName}:`, error.message);
      throw error;
    }
  }

  /**
   * Wait for transaction and get receipt
   */
  async waitForTransaction(tx, confirmations = 1) {
    const receipt = await tx.wait(confirmations);

    return {
      receipt,
      gasUsed: receipt.gasUsed,
      gasPrice: receipt.gasPrice || tx.gasPrice,
      cost: (receipt.gasUsed * (receipt.gasPrice || tx.gasPrice)).toString(),
      status: receipt.status === 1 ? 'success' : 'failed',
    };
  }

  /**
   * Parse events from transaction receipt
   */
  parseEvents(receipt, contract) {
    const events = [];

    for (const log of receipt.logs) {
      try {
        const parsed = contract.interface.parseLog(log);
        if (parsed) {
          events.push({
            name: parsed.name,
            args: parsed.args,
            signature: parsed.signature,
            topic: parsed.topic,
          });
        }
      } catch (error) {
        // Log doesn't match this contract's interface
      }
    }

    return events;
  }

  /**
   * Advanced contract interaction with error handling
   */
  async callContract(contract, methodName, args = [], options = {}) {
    try {
      const method = contract[methodName];

      if (options.staticCall) {
        // Read-only call
        return await method.staticCall(...args);
      } else {
        // State-changing transaction
        const tx = await method(...args, options.txOptions || {});
        const result = await this.waitForTransaction(tx);

        // Parse events if contract provided
        if (options.parseEvents) {
          result.events = this.parseEvents(result.receipt, contract);
        }

        return result;
      }
    } catch (error) {
      console.error(`Contract call failed - ${methodName}:`, error.message);
      throw error;
    }
  }

  /**
   * Test contract upgrade
   */
  async testContractUpgrade(proxyAddress, newImplementation) {
    // This is a simplified upgrade test
    // In practice, you'd use a proper proxy pattern

    const oldImplementation = await ethers.getContractAt(
      'Contract',
      proxyAddress
    );

    // Deploy new implementation
    const NewContract = await ethers.getContractFactory(newImplementation);
    const newContract = await NewContract.deploy();
    await newContract.waitForDeployment();

    return {
      oldAddress: proxyAddress,
      newAddress: await newContract.getAddress(),
      oldContract: oldImplementation,
      newContract,
    };
  }

  /**
   * Generate test report for contract tests
   */
  generateContractTestReport(testResults) {
    const report = {
      timestamp: new Date().toISOString(),
      totalTests: testResults.length,
      passed: testResults.filter(r => r.passed).length,
      failed: testResults.filter(r => !r.passed).length,
      gasUsage: {
        total: testResults.reduce((sum, r) => sum + (r.gasUsed || 0), 0),
        average: 0,
        max: Math.max(...testResults.map(r => r.gasUsed || 0)),
        min: Math.min(...testResults.map(r => r.gasUsed || 0)),
      },
      deployedContracts: Array.from(this.deployedContracts.keys()),
      recommendations: [],
    };

    report.gasUsage.average = report.gasUsage.total / testResults.length;

    // Generate recommendations based on gas usage
    if (report.gasUsage.max > 500000) {
      report.recommendations.push(
        'Consider optimizing high gas usage transactions'
      );
    }

    if (report.gasUsage.average > 100000) {
      report.recommendations.push(
        'Average gas usage is high, review contract efficiency'
      );
    }

    return report;
  }

  /**
   * Cleanup test environment
   */
  async cleanup() {
    // Restore to initial snapshot if available
    if (this.snapshots.length > 0) {
      await this.restoreSnapshot(this.snapshots[0]);
    }

    // Clear deployed contracts cache
    this.deployedContracts.clear();
    this.snapshots.length = 0;
  }
}

// Global helper instance
const globalHardhatHelper = new HardhatTestHelper();

// Common test utilities
const testHelpers = {
  /**
   * Test expectation helpers
   */
  expectEvent: (receipt, eventName) => {
    const events = globalHardhatHelper.parseEvents(receipt, receipt.contract);
    const event = events.find(e => e.name === eventName);
    expect(event).to.exist;
    return event;
  },

  expectRevert: async (promise, errorMessage) => {
    try {
      await promise;
      expect.fail('Expected transaction to revert');
    } catch (error) {
      if (errorMessage) {
        expect(error.message).to.include(errorMessage);
      }
    }
  },

  /**
   * Token amount helpers
   */
  parseTokens: (amount, decimals = 18) => {
    return ethers.parseUnits(amount.toString(), decimals);
  },

  formatTokens: (amount, decimals = 18) => {
    return ethers.formatUnits(amount, decimals);
  },

  /**
   * Time helpers
   */
  timeTravel: async seconds => {
    return globalHardhatHelper.increaseTime(seconds);
  },

  latest: async () => {
    const block = await globalHardhatHelper.getLatestBlock();
    return block.timestamp;
  },
};

module.exports = {
  HardhatTestHelper,
  globalHardhatHelper,
  testHelpers,

  // Export commonly used functions
  deployContract: (name, args, options) =>
    globalHardhatHelper.deployContract(name, args, options),
  takeSnapshot: () => globalHardhatHelper.takeSnapshot(),
  restoreSnapshot: id => globalHardhatHelper.restoreSnapshot(id),
  mineBlocks: count => globalHardhatHelper.mineBlocks(count),
  increaseTime: seconds => globalHardhatHelper.increaseTime(seconds),
  setup: () => globalHardhatHelper.setup(),
  cleanup: () => globalHardhatHelper.cleanup(),
};
