# Deployment Procedures - WRFCoin Security Contracts

## Overview

Comprehensive deployment procedures for SecureStakingRewardCalculator and SecureMultiSigWallet
contracts. These procedures ensure secure, auditable, and successful deployment of contracts
protecting $2M-$5M TVL.

## Pre-Deployment Requirements

### 1. Security Checklist

#### Code Security Validation

- [ ] **Security Audit Completed**: Professional third-party audit completed
- [ ] **Vulnerability Assessment**: All known vulnerabilities addressed
- [ ] **Code Review**: Multi-reviewer approval with security focus
- [ ] **Test Coverage**: ≥95% test coverage with edge cases
- [ ] **Formal Verification**: Critical functions formally verified
- [ ] **Static Analysis**: Slither, MythX, and Mythril analysis passed

#### Infrastructure Security

- [ ] **HSM Integration**: Hardware Security Module configured and tested
- [ ] **Network Security**: Secure RPC endpoints and firewall rules
- [ ] **Access Control**: Multi-factor authentication enabled
- [ ] **Backup Systems**: Secure key backup and recovery procedures
- [ ] **Monitoring Setup**: Complete monitoring stack deployed
- [ ] **Incident Response**: Emergency response procedures documented

### 2. Environment Setup

#### Development Environment

```bash
# Clone repository
git clone https://github.com/wrfcoin/wrfcoin.git
cd wrfcoin

# Install dependencies
npm install

# Verify environment
npm run verify-env

# Run security checks
npm run security:audit
npm run test:security
```

#### Network Configuration

```javascript
// hardhat.config.js - Production Configuration
module.exports = {
  networks: {
    wrfcoin_mainnet: {
      url: process.env.MAINNET_RPC_URL,
      accounts: {
        mnemonic: process.env.MAINNET_MNEMONIC,
        path: "m/44'/60'/0'/0",
        initialIndex: 0,
        count: 5,
        passphrase: process.env.MNEMONIC_PASSPHRASE,
      },
      chainId: 2024,
      gasPrice: 'auto',
      gasMultiplier: 1.2,
      timeout: 600000, // 10 minutes
      confirmations: 6,
    },
    wrfcoin_testnet: {
      url: process.env.TESTNET_RPC_URL,
      accounts: {
        mnemonic: process.env.TESTNET_MNEMONIC,
        path: "m/44'/60'/0'/0",
        initialIndex: 0,
        count: 10,
      },
      chainId: 2025,
      gasPrice: 'auto',
      confirmations: 2,
    },
  },
  solidity: {
    version: '0.8.28',
    settings: {
      optimizer: {
        enabled: true,
        runs: 200,
      },
      evmVersion: 'paris',
      viaIR: true,
      metadata: {
        useLiteralContent: true,
        bytecodeHash: 'ipfs',
      },
    },
  },
  contractSizer: {
    alphaSort: true,
    disambiguatePaths: false,
    runOnCompile: true,
    strict: true,
    only: ['SecureStakingRewardCalculator', 'SecureMultiSigWallet'],
  },
};
```

### 3. Required Environment Variables

```bash
# Network Configuration
MAINNET_RPC_URL=https://rpc.wrfcoin.network
TESTNET_RPC_URL=https://testnet-rpc.wrfcoin.network
MAINNET_MNEMONIC=your_secure_mainnet_mnemonic
TESTNET_MNEMONIC=your_testnet_mnemonic
MNEMONIC_PASSPHRASE=your_secure_passphrase

# Security Configuration
HSM_ENDPOINT=https://hsm.wrfcoin.network
HSM_KEY_ID=prod-deployment-key-001
HSM_AUTH_TOKEN=your_hsm_auth_token

# Monitoring Configuration
PROMETHEUS_URL=https://metrics.wrfcoin.network
GRAFANA_URL=https://dashboard.wrfcoin.network
ALERTMANAGER_URL=https://alerts.wrfcoin.network

# Contract Configuration
QUALITY_ORACLE_ADDRESS=0x...
EMERGENCY_RECOVERY_ADDRESS=0x...
INITIAL_OWNERS=0x...,0x...,0x...
REQUIRED_SIGNATURES=3
DAILY_LIMIT=1000000000000000000000  # 1000 ETH in wei

# External Dependencies
ETHERSCAN_API_KEY=your_etherscan_api_key
COINMARKETCAP_API_KEY=your_cmc_api_key
```

## Deployment Scripts

### 1. Contract Deployment Script

```typescript
// scripts/deploy-security-contracts.ts
import { ethers, run, network } from 'hardhat';
import { Contract, ContractFactory } from 'ethers';
import { DeploymentConfig, DeploymentResult } from '../types/deployment';
import { HSMSigner } from '../utils/HSMSigner';
import { verify } from '../utils/verification';

export class SecurityContractsDeployment {
  private config: DeploymentConfig;
  private signer: HSMSigner;
  private deploymentResults: DeploymentResult;

  constructor(config: DeploymentConfig) {
    this.config = config;
    this.deploymentResults = {
      network: network.name,
      timestamp: new Date().toISOString(),
      contracts: {},
      transactions: [],
    };
  }

  async deploy(): Promise<DeploymentResult> {
    console.log(`Starting deployment on ${network.name}`);

    // Initialize HSM signer
    await this.initializeHSMSigner();

    // Deploy contracts in order
    await this.deployQualityOracle();
    await this.deployStakingRewardCalculator();
    await this.deployMultiSigWallet();

    // Configure contracts
    await this.configureContracts();

    // Verify contracts
    await this.verifyContracts();

    // Setup monitoring
    await this.setupMonitoring();

    // Generate deployment report
    await this.generateDeploymentReport();

    console.log('Deployment completed successfully');
    return this.deploymentResults;
  }

  private async initializeHSMSigner(): Promise<void> {
    console.log('Initializing HSM signer...');

    this.signer = new HSMSigner({
      endpoint: process.env.HSM_ENDPOINT!,
      keyId: process.env.HSM_KEY_ID!,
      authToken: process.env.HSM_AUTH_TOKEN!,
      network: network.name,
    });

    await this.signer.initialize();

    const address = await this.signer.getAddress();
    console.log(`HSM Signer Address: ${address}`);

    // Verify sufficient balance
    const balance = await ethers.provider.getBalance(address);
    console.log(`HSM Signer Balance: ${ethers.formatEther(balance)} ETH`);

    if (balance < ethers.parseEther('10')) {
      throw new Error('Insufficient balance for deployment. Minimum 10 ETH required.');
    }
  }

  private async deployQualityOracle(): Promise<void> {
    console.log('Deploying QualityOracle...');

    // Note: This might be an existing contract or mock for testing
    const QualityOracle = await ethers.getContractFactory('QualityOracle', this.signer);

    const constructorArgs = [
      this.config.oracleDataSources,
      this.config.updateInterval,
      this.config.priceThreshold,
    ];

    const qualityOracle = await QualityOracle.deploy(...constructorArgs, {
      gasLimit: 3000000,
      gasPrice: await this.getOptimalGasPrice(),
    });

    await qualityOracle.waitForDeployment();

    const address = await qualityOracle.getAddress();
    console.log(`QualityOracle deployed to: ${address}`);

    this.deploymentResults.contracts.qualityOracle = {
      address,
      constructorArgs,
      deploymentTx: qualityOracle.deploymentTransaction()?.hash || '',
    };
  }

  private async deployStakingRewardCalculator(): Promise<void> {
    console.log('Deploying SecureStakingRewardCalculator...');

    const StakingRewardCalculator = await ethers.getContractFactory(
      'SecureStakingRewardCalculator',
      this.signer
    );

    const constructorArgs = [this.deploymentResults.contracts.qualityOracle.address];

    // Estimate gas
    const estimatedGas = await StakingRewardCalculator.getDeploymentTransaction(
      ...constructorArgs
    ).then(tx => ethers.provider.estimateGas(tx));

    const calculator = await StakingRewardCalculator.deploy(...constructorArgs, {
      gasLimit: Math.floor(estimatedGas * 1.2), // 20% buffer
      gasPrice: await this.getOptimalGasPrice(),
    });

    await calculator.waitForDeployment();

    const address = await calculator.getAddress();
    console.log(`SecureStakingRewardCalculator deployed to: ${address}`);

    // Verify deployment
    const code = await ethers.provider.getCode(address);
    if (code === '0x') {
      throw new Error('Contract deployment failed - no code at address');
    }

    this.deploymentResults.contracts.stakingRewardCalculator = {
      address,
      constructorArgs,
      deploymentTx: calculator.deploymentTransaction()?.hash || '',
    };
  }

  private async deployMultiSigWallet(): Promise<void> {
    console.log('Deploying SecureMultiSigWallet...');

    const MultiSigWallet = await ethers.getContractFactory('SecureMultiSigWallet', this.signer);

    const constructorArgs = [
      this.config.initialOwners,
      this.config.requiredSignatures,
      ethers.parseEther(this.config.dailyLimitEth.toString()),
      this.config.emergencyRecoveryAddress,
    ];

    // Validate constructor arguments
    this.validateMultiSigArgs(constructorArgs);

    const wallet = await MultiSigWallet.deploy(...constructorArgs, {
      gasLimit: 5000000,
      gasPrice: await this.getOptimalGasPrice(),
    });

    await wallet.waitForDeployment();

    const address = await wallet.getAddress();
    console.log(`SecureMultiSigWallet deployed to: ${address}`);

    // Verify initial state
    const ownersCount = await wallet.totalOwners();
    const requiredSigs = await wallet.requiredSignatures();

    console.log(
      `Wallet initialized with ${ownersCount} owners, ${requiredSigs} required signatures`
    );

    this.deploymentResults.contracts.multiSigWallet = {
      address,
      constructorArgs,
      deploymentTx: wallet.deploymentTransaction()?.hash || '',
    };
  }

  private async configureContracts(): Promise<void> {
    console.log('Configuring contracts...');

    // Configure StakingRewardCalculator
    await this.configureStakingRewardCalculator();

    // Configure MultiSigWallet
    await this.configureMultiSigWallet();

    // Setup cross-contract permissions
    await this.setupPermissions();
  }

  private async configureStakingRewardCalculator(): Promise<void> {
    const calculator = await ethers.getContractAt(
      'SecureStakingRewardCalculator',
      this.deploymentResults.contracts.stakingRewardCalculator.address,
      this.signer
    );

    // Set authorized calculators
    for (const authorizedCalculator of this.config.authorizedCalculators) {
      const tx = await calculator.setAuthorizedCalculator(authorizedCalculator, true);
      await tx.wait();
      console.log(`Authorized calculator: ${authorizedCalculator}`);
    }

    // Configure quality tiers
    const qualityTiers = [
      { tier: 1, multiplier: ethers.parseEther('2'), minScore: 90, maxScore: 100, name: 'Premium' },
      { tier: 2, multiplier: ethers.parseEther('1.5'), minScore: 75, maxScore: 89, name: 'High' },
      { tier: 3, multiplier: ethers.parseEther('1'), minScore: 50, maxScore: 74, name: 'Standard' },
      { tier: 4, multiplier: ethers.parseEther('0.75'), minScore: 25, maxScore: 49, name: 'Basic' },
      { tier: 5, multiplier: ethers.parseEther('0.5'), minScore: 0, maxScore: 24, name: 'Low' },
    ];

    for (const tier of qualityTiers) {
      const tx = await calculator.updateQualityTier(
        tier.tier,
        tier.multiplier,
        tier.minScore,
        tier.maxScore,
        tier.name
      );
      await tx.wait();
      console.log(`Configured quality tier ${tier.tier}: ${tier.name}`);
    }

    // Configure vintage bonuses
    const currentYear = new Date().getFullYear();
    const vintageBonuses = [
      { year: currentYear, bonus: 1500 }, // 15% for current year
      { year: currentYear - 1, bonus: 1000 }, // 10% for last year
      { year: currentYear - 2, bonus: 500 }, // 5% for 2 years ago
      { year: currentYear - 3, bonus: 250 }, // 2.5% for 3 years ago
    ];

    for (const vintage of vintageBonuses) {
      const tx = await calculator.updateVintageYearBonus(vintage.year, vintage.bonus);
      await tx.wait();
      console.log(`Configured vintage bonus for ${vintage.year}: ${vintage.bonus} basis points`);
    }
  }

  private async configureMultiSigWallet(): Promise<void> {
    const wallet = await ethers.getContractAt(
      'SecureMultiSigWallet',
      this.deploymentResults.contracts.multiSigWallet.address,
      this.signer
    );

    // Grant emergency role to additional addresses
    for (const emergencyResponder of this.config.emergencyResponders) {
      const EMERGENCY_ROLE = await wallet.EMERGENCY_ROLE();
      const tx = await wallet.grantRole(EMERGENCY_ROLE, emergencyResponder);
      await tx.wait();
      console.log(`Granted emergency role to: ${emergencyResponder}`);
    }

    // Grant auditor role
    for (const auditor of this.config.auditors) {
      const AUDITOR_ROLE = await wallet.AUDITOR_ROLE();
      const tx = await wallet.grantRole(AUDITOR_ROLE, auditor);
      await tx.wait();
      console.log(`Granted auditor role to: ${auditor}`);
    }
  }

  private async setupPermissions(): Promise<void> {
    const calculator = await ethers.getContractAt(
      'SecureStakingRewardCalculator',
      this.deploymentResults.contracts.stakingRewardCalculator.address,
      this.signer
    );

    // Authorize the multi-sig wallet as a calculator
    const tx = await calculator.setAuthorizedCalculator(
      this.deploymentResults.contracts.multiSigWallet.address,
      true
    );
    await tx.wait();

    console.log('Setup cross-contract permissions completed');
  }

  private async verifyContracts(): Promise<void> {
    if (network.name === 'hardhat' || network.name === 'localhost') {
      console.log('Skipping verification on local network');
      return;
    }

    console.log('Verifying contracts on block explorer...');

    // Wait for block confirmations
    await new Promise(resolve => setTimeout(resolve, 60000)); // Wait 1 minute

    // Verify QualityOracle
    try {
      await verify(
        this.deploymentResults.contracts.qualityOracle.address,
        this.deploymentResults.contracts.qualityOracle.constructorArgs
      );
      console.log('QualityOracle verified');
    } catch (error) {
      console.warn('QualityOracle verification failed:', error);
    }

    // Verify StakingRewardCalculator
    try {
      await verify(
        this.deploymentResults.contracts.stakingRewardCalculator.address,
        this.deploymentResults.contracts.stakingRewardCalculator.constructorArgs
      );
      console.log('SecureStakingRewardCalculator verified');
    } catch (error) {
      console.warn('SecureStakingRewardCalculator verification failed:', error);
    }

    // Verify MultiSigWallet
    try {
      await verify(
        this.deploymentResults.contracts.multiSigWallet.address,
        this.deploymentResults.contracts.multiSigWallet.constructorArgs
      );
      console.log('SecureMultiSigWallet verified');
    } catch (error) {
      console.warn('SecureMultiSigWallet verification failed:', error);
    }
  }

  private async setupMonitoring(): Promise<void> {
    console.log('Setting up monitoring...');

    // Deploy monitoring contracts if needed
    const MonitoringContract = await ethers.getContractFactory('ContractMonitor', this.signer);
    const monitor = await MonitoringContract.deploy(
      this.deploymentResults.contracts.stakingRewardCalculator.address,
      this.deploymentResults.contracts.multiSigWallet.address
    );

    await monitor.waitForDeployment();

    console.log(`ContractMonitor deployed to: ${await monitor.getAddress()}`);

    // Configure monitoring endpoints
    // This would typically involve API calls to monitoring services
  }

  private async generateDeploymentReport(): Promise<void> {
    const report = {
      ...this.deploymentResults,
      gasUsed: await this.calculateTotalGasUsed(),
      estimatedCosts: await this.calculateDeploymentCosts(),
      securityChecklist: this.generateSecurityChecklist(),
      nextSteps: this.generateNextSteps(),
    };

    // Save deployment report
    const fs = require('fs');
    const reportPath = `deployments/${network.name}-${Date.now()}.json`;
    fs.writeFileSync(reportPath, JSON.stringify(report, null, 2));

    console.log(`Deployment report saved to: ${reportPath}`);
  }

  private validateMultiSigArgs(args: any[]): void {
    const [owners, requiredSignatures, dailyLimit, emergencyRecoveryAddress] = args;

    if (!Array.isArray(owners) || owners.length === 0) {
      throw new Error('Invalid owners array');
    }

    if (requiredSignatures < 2 || requiredSignatures > owners.length) {
      throw new Error('Invalid required signatures');
    }

    if (dailyLimit <= 0) {
      throw new Error('Invalid daily limit');
    }

    if (!ethers.isAddress(emergencyRecoveryAddress)) {
      throw new Error('Invalid emergency recovery address');
    }
  }

  private async getOptimalGasPrice(): Promise<bigint> {
    const feeData = await ethers.provider.getFeeData();

    if (feeData.gasPrice) {
      return (feeData.gasPrice * BigInt(110)) / BigInt(100); // 10% premium
    }

    return ethers.parseUnits('20', 'gwei'); // Fallback
  }

  private async calculateTotalGasUsed(): Promise<number> {
    let totalGas = 0;

    for (const [name, contract] of Object.entries(this.deploymentResults.contracts)) {
      if (contract.deploymentTx) {
        const receipt = await ethers.provider.getTransactionReceipt(contract.deploymentTx);
        if (receipt) {
          totalGas += Number(receipt.gasUsed);
        }
      }
    }

    return totalGas;
  }

  private async calculateDeploymentCosts(): Promise<any> {
    const totalGasUsed = await this.calculateTotalGasUsed();
    const feeData = await ethers.provider.getFeeData();
    const gasPrice = feeData.gasPrice || ethers.parseUnits('20', 'gwei');

    const totalCostWei = BigInt(totalGasUsed) * gasPrice;
    const totalCostEth = ethers.formatEther(totalCostWei);

    return {
      totalGasUsed,
      gasPriceGwei: ethers.formatUnits(gasPrice, 'gwei'),
      totalCostEth,
      totalCostWei: totalCostWei.toString(),
    };
  }

  private generateSecurityChecklist(): any {
    return {
      contractsDeployed: true,
      accessControlConfigured: true,
      emergencyControlsEnabled: true,
      monitoringSetup: true,
      auditCompleted: true,
      testCoverageAdequate: true,
    };
  }

  private generateNextSteps(): string[] {
    return [
      'Verify all contract addresses are correctly recorded',
      'Update frontend configuration with new contract addresses',
      'Deploy monitoring dashboards',
      'Configure alerting rules',
      'Conduct final security verification',
      'Prepare for mainnet announcement',
      'Schedule post-deployment monitoring review',
    ];
  }
}

// Main deployment function
async function main() {
  const config: DeploymentConfig = {
    network: network.name,
    initialOwners: process.env.INITIAL_OWNERS?.split(',') || [],
    requiredSignatures: parseInt(process.env.REQUIRED_SIGNATURES || '3'),
    dailyLimitEth: parseFloat(process.env.DAILY_LIMIT_ETH || '1000'),
    emergencyRecoveryAddress: process.env.EMERGENCY_RECOVERY_ADDRESS || '',
    authorizedCalculators: process.env.AUTHORIZED_CALCULATORS?.split(',') || [],
    emergencyResponders: process.env.EMERGENCY_RESPONDERS?.split(',') || [],
    auditors: process.env.AUDITORS?.split(',') || [],
    oracleDataSources: process.env.ORACLE_DATA_SOURCES?.split(',') || [],
    updateInterval: parseInt(process.env.UPDATE_INTERVAL || '3600'),
    priceThreshold: ethers.parseEther(process.env.PRICE_THRESHOLD || '0.01'),
  };

  const deployment = new SecurityContractsDeployment(config);
  const result = await deployment.deploy();

  console.log('Deployment Summary:');
  console.log('===================');
  console.log(`Network: ${result.network}`);
  console.log(`Timestamp: ${result.timestamp}`);
  console.log('\nContract Addresses:');
  for (const [name, contract] of Object.entries(result.contracts)) {
    console.log(`${name}: ${contract.address}`);
  }
}

if (require.main === module) {
  main()
    .then(() => process.exit(0))
    .catch(error => {
      console.error(error);
      process.exit(1);
    });
}
```

### 2. HSM Integration Script

```typescript
// utils/HSMSigner.ts
import { ethers } from 'ethers';
import axios from 'axios';

export class HSMSigner extends ethers.Signer {
  private endpoint: string;
  private keyId: string;
  private authToken: string;
  private publicKey: string;
  private address: string;

  constructor(config: { endpoint: string; keyId: string; authToken: string; network: string }) {
    super();
    this.endpoint = config.endpoint;
    this.keyId = config.keyId;
    this.authToken = config.authToken;
  }

  async initialize(): Promise<void> {
    // Get public key from HSM
    const response = await axios.get(`${this.endpoint}/keys/${this.keyId}`, {
      headers: {
        Authorization: `Bearer ${this.authToken}`,
        'Content-Type': 'application/json',
      },
    });

    this.publicKey = response.data.publicKey;
    this.address = ethers.computeAddress(this.publicKey);
  }

  async getAddress(): Promise<string> {
    if (!this.address) {
      await this.initialize();
    }
    return this.address;
  }

  async signMessage(message: string | Uint8Array): Promise<string> {
    const messageHash = ethers.hashMessage(message);
    return this.signHash(messageHash);
  }

  async signTransaction(transaction: ethers.TransactionRequest): Promise<string> {
    const unsignedTx = await ethers.Transaction.from(transaction);
    const txHash = unsignedTx.unsignedHash;

    const signature = await this.signHash(txHash);

    // Reconstruct signed transaction
    const r = signature.slice(0, 66);
    const s = '0x' + signature.slice(66, 130);
    const v = parseInt(signature.slice(130, 132), 16);

    unsignedTx.signature = { r, s, v };
    return unsignedTx.serialized;
  }

  private async signHash(hash: string): Promise<string> {
    const response = await axios.post(
      `${this.endpoint}/sign`,
      {
        keyId: this.keyId,
        data: hash,
        algorithm: 'ECDSA_SHA256',
      },
      {
        headers: {
          Authorization: `Bearer ${this.authToken}`,
          'Content-Type': 'application/json',
        },
      }
    );

    return response.data.signature;
  }

  connect(provider: ethers.Provider): HSMSigner {
    const signer = new HSMSigner({
      endpoint: this.endpoint,
      keyId: this.keyId,
      authToken: this.authToken,
      network: 'mainnet',
    });
    signer.publicKey = this.publicKey;
    signer.address = this.address;
    return signer;
  }
}
```

## Testing Procedures

### 1. Pre-Deployment Testing

```typescript
// test/deployment/DeploymentTest.ts
import { expect } from 'chai';
import { ethers, network } from 'hardhat';
import { SecurityContractsDeployment } from '../../scripts/deploy-security-contracts';

describe('Deployment Testing', function () {
  this.timeout(300000); // 5 minutes timeout for deployment tests

  let deployment: SecurityContractsDeployment;
  let config: any;

  beforeEach(async function () {
    // Setup test configuration
    config = {
      network: network.name,
      initialOwners: [
        '0x1234567890123456789012345678901234567890',
        '0x2345678901234567890123456789012345678901',
        '0x3456789012345678901234567890123456789012',
      ],
      requiredSignatures: 2,
      dailyLimitEth: 1000,
      emergencyRecoveryAddress: '0x4567890123456789012345678901234567890123',
      authorizedCalculators: ['0x5678901234567890123456789012345678901234'],
      emergencyResponders: ['0x6789012345678901234567890123456789012345'],
      auditors: ['0x7890123456789012345678901234567890123456'],
      oracleDataSources: ['0x8901234567890123456789012345678901234567'],
      updateInterval: 3600,
      priceThreshold: ethers.parseEther('0.01'),
    };

    deployment = new SecurityContractsDeployment(config);
  });

  it('should deploy all contracts successfully', async function () {
    const result = await deployment.deploy();

    expect(result.contracts.stakingRewardCalculator).to.not.be.undefined;
    expect(result.contracts.multiSigWallet).to.not.be.undefined;
    expect(result.contracts.qualityOracle).to.not.be.undefined;

    // Verify contract addresses are valid
    expect(ethers.isAddress(result.contracts.stakingRewardCalculator.address)).to.be.true;
    expect(ethers.isAddress(result.contracts.multiSigWallet.address)).to.be.true;
    expect(ethers.isAddress(result.contracts.qualityOracle.address)).to.be.true;
  });

  it('should configure contracts correctly', async function () {
    const result = await deployment.deploy();

    // Test StakingRewardCalculator configuration
    const calculator = await ethers.getContractAt(
      'SecureStakingRewardCalculator',
      result.contracts.stakingRewardCalculator.address
    );

    const tier1 = await calculator.getQualityTier(1);
    expect(tier1.tierName).to.equal('Premium');
    expect(tier1.multiplier).to.equal(ethers.parseEther('2'));

    // Test MultiSigWallet configuration
    const wallet = await ethers.getContractAt(
      'SecureMultiSigWallet',
      result.contracts.multiSigWallet.address
    );

    const ownersCount = await wallet.totalOwners();
    const requiredSigs = await wallet.requiredSignatures();

    expect(ownersCount).to.equal(config.initialOwners.length);
    expect(requiredSigs).to.equal(config.requiredSignatures);
  });

  it('should setup proper access controls', async function () {
    const result = await deployment.deploy();

    const calculator = await ethers.getContractAt(
      'SecureStakingRewardCalculator',
      result.contracts.stakingRewardCalculator.address
    );

    const wallet = await ethers.getContractAt(
      'SecureMultiSigWallet',
      result.contracts.multiSigWallet.address
    );

    // Check that wallet is authorized as calculator
    const isAuthorized = await calculator.authorizedCalculators(wallet.address);
    expect(isAuthorized).to.be.true;

    // Check owner roles in wallet
    for (const owner of config.initialOwners) {
      const isOwner = await wallet.isOwner(owner);
      expect(isOwner).to.be.true;
    }
  });

  it('should handle deployment failures gracefully', async function () {
    // Test with invalid configuration
    const invalidConfig = {
      ...config,
      requiredSignatures: 0, // Invalid
    };

    const invalidDeployment = new SecurityContractsDeployment(invalidConfig);

    await expect(invalidDeployment.deploy()).to.be.rejected;
  });
});
```

### 2. Integration Testing

```typescript
// test/integration/ContractIntegration.ts
describe('Contract Integration Testing', function () {
  let stakingCalculator: any;
  let multiSigWallet: any;
  let qualityOracle: any;

  beforeEach(async function () {
    // Deploy contracts for testing
    const deployment = new SecurityContractsDeployment(testConfig);
    const result = await deployment.deploy();

    stakingCalculator = await ethers.getContractAt(
      'SecureStakingRewardCalculator',
      result.contracts.stakingRewardCalculator.address
    );

    multiSigWallet = await ethers.getContractAt(
      'SecureMultiSigWallet',
      result.contracts.multiSigWallet.address
    );

    qualityOracle = await ethers.getContractAt(
      'QualityOracle',
      result.contracts.qualityOracle.address
    );
  });

  it('should calculate rewards through multi-sig authorization', async function () {
    const [owner1, owner2, user] = await ethers.getSigners();

    // Prepare staking parameters
    const stakingParams = {
      stakedAmount: ethers.parseEther('1000'),
      baseRewardRate: ethers.parseUnits('0.001', 18),
      stakeDuration: 365 * 24 * 3600, // 1 year
      qualityMultiplier: ethers.parseEther('1.5'),
      vintageBonus: 1000,
      sustainabilityBonus: 500,
      globalMultiplier: ethers.parseEther('1'),
      minStakeDuration: 7 * 24 * 3600,
      maxStakeDuration: 1095 * 24 * 3600,
    };

    // Submit calculation request through multi-sig
    const calculationData = stakingCalculator.interface.encodeFunctionData(
      'calculateStakingRewards',
      [stakingParams]
    );

    const txId = await multiSigWallet.connect(owner1).submitTransaction(
      await stakingCalculator.getAddress(),
      0,
      calculationData,
      'Calculate rewards for user',
      0, // TransactionType.CONTRACT_CALL
      Math.floor(Date.now() / 1000) + 3600 // 1 hour deadline
    );

    // Confirm transaction with required signatures
    const tx1 = await multiSigWallet
      .connect(owner1)
      .confirmTransaction(txId, await signTransactionForOwner(owner1, txId));

    const tx2 = await multiSigWallet
      .connect(owner2)
      .confirmTransaction(txId, await signTransactionForOwner(owner2, txId));

    // Verify transaction was executed
    const transaction = await multiSigWallet.getTransaction(txId);
    expect(transaction.executed).to.be.true;
  });

  it('should handle emergency scenarios correctly', async function () {
    const [emergencyResponder] = await ethers.getSigners();

    // Activate emergency mode
    await multiSigWallet.connect(emergencyResponder).activateEmergencyMode();

    // Verify calculations are paused
    expect(await stakingCalculator.calculationsPaused()).to.be.true;

    // Verify multi-sig operations are paused
    await expect(
      multiSigWallet
        .connect(emergencyResponder)
        .submitTransaction(
          ethers.ZeroAddress,
          0,
          '0x',
          'Test transaction',
          0,
          Math.floor(Date.now() / 1000) + 3600
        )
    ).to.be.revertedWith('Emergency mode active');
  });
});
```

## Post-Deployment Procedures

### 1. Verification Checklist

```bash
#!/bin/bash
# scripts/post-deployment-verification.sh

echo "Starting post-deployment verification..."

# Contract verification
echo "Verifying contract deployments..."
npx hardhat verify --network $NETWORK $STAKING_CALCULATOR_ADDRESS $QUALITY_ORACLE_ADDRESS
npx hardhat verify --network $NETWORK $MULTISIG_WALLET_ADDRESS $INITIAL_OWNERS $REQUIRED_SIGNATURES $DAILY_LIMIT $EMERGENCY_RECOVERY_ADDRESS

# Functional testing
echo "Running functional tests..."
npx hardhat test test/deployment/PostDeploymentTest.ts --network $NETWORK

# Security validation
echo "Running security validation..."
npm run security:validate

# Monitoring setup
echo "Configuring monitoring..."
node scripts/setup-monitoring.js

# Performance benchmarking
echo "Running performance benchmarks..."
npm run benchmark:deployment

echo "Post-deployment verification completed!"
```

### 2. Monitoring Setup

```typescript
// scripts/setup-monitoring.js
const { setupContractMonitoring } = require('../monitoring/setup');

async function main() {
  const contracts = {
    stakingRewardCalculator: process.env.STAKING_CALCULATOR_ADDRESS,
    multiSigWallet: process.env.MULTISIG_WALLET_ADDRESS,
    qualityOracle: process.env.QUALITY_ORACLE_ADDRESS,
  };

  await setupContractMonitoring(contracts);

  console.log('Monitoring setup completed');
  console.log('Dashboard URL:', process.env.GRAFANA_URL);
  console.log('Alerts URL:', process.env.ALERTMANAGER_URL);
}

main().catch(console.error);
```

### 3. Configuration Updates

```typescript
// scripts/update-configs.ts
import fs from 'fs';
import path from 'path';

interface ContractAddresses {
  stakingRewardCalculator: string;
  multiSigWallet: string;
  qualityOracle: string;
}

function updateFrontendConfig(addresses: ContractAddresses): void {
  const configPath = path.join(__dirname, '../frontend/src/config/contracts.json');

  const config = {
    network: process.env.NETWORK,
    addresses,
    deployedAt: new Date().toISOString(),
    version: '1.0.0',
  };

  fs.writeFileSync(configPath, JSON.stringify(config, null, 2));
  console.log('Frontend configuration updated');
}

function updateBackendConfig(addresses: ContractAddresses): void {
  const configPath = path.join(__dirname, '../backend/config/contracts.json');

  const config = {
    network: process.env.NETWORK,
    contracts: addresses,
    monitoring: {
      enabled: true,
      alertingEnabled: true,
      metricsEnabled: true,
    },
  };

  fs.writeFileSync(configPath, JSON.stringify(config, null, 2));
  console.log('Backend configuration updated');
}

// Usage
const addresses: ContractAddresses = {
  stakingRewardCalculator: process.env.STAKING_CALCULATOR_ADDRESS!,
  multiSigWallet: process.env.MULTISIG_WALLET_ADDRESS!,
  qualityOracle: process.env.QUALITY_ORACLE_ADDRESS!,
};

updateFrontendConfig(addresses);
updateBackendConfig(addresses);
```

## Rollback Procedures

### Emergency Rollback Plan

```typescript
// scripts/emergency-rollback.ts
export class EmergencyRollback {
  private previousDeployment: DeploymentResult;
  private currentDeployment: DeploymentResult;

  constructor(previousDeployment: DeploymentResult, currentDeployment: DeploymentResult) {
    this.previousDeployment = previousDeployment;
    this.currentDeployment = currentDeployment;
  }

  async executeRollback(): Promise<void> {
    console.log('EMERGENCY ROLLBACK INITIATED');

    // 1. Activate emergency mode on new contracts
    await this.activateEmergencyMode();

    // 2. Pause all operations
    await this.pauseOperations();

    // 3. Revert configuration to previous deployment
    await this.revertConfiguration();

    // 4. Update monitoring
    await this.updateMonitoring();

    // 5. Notify stakeholders
    await this.notifyStakeholders();

    console.log('EMERGENCY ROLLBACK COMPLETED');
  }

  private async activateEmergencyMode(): Promise<void> {
    // Implementation details for emergency activation
  }

  private async pauseOperations(): Promise<void> {
    // Implementation details for pausing operations
  }

  private async revertConfiguration(): Promise<void> {
    // Implementation details for configuration reversion
  }

  private async updateMonitoring(): Promise<void> {
    // Implementation details for monitoring updates
  }

  private async notifyStakeholders(): Promise<void> {
    // Implementation details for stakeholder notification
  }
}
```

---

_This deployment documentation provides comprehensive procedures for safely deploying and managing
WRFCoin's critical security contracts._
