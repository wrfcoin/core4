# Cross-Chain Bridge Integration Guide

Complete guide to integrating WRFCoin's cross-chain bridges for seamless interoperability with
Ethereum, Polygon, and other major blockchain networks.

## What You'll Learn

- Cross-chain bridge architecture and protocols
- Asset bridging and liquidity management
- Cross-chain transaction monitoring
- Multi-signature security implementations
- Validator network integration
- Bridge operation and maintenance

## Prerequisites

- Completed [Smart Contract Deployment](../smart-contracts/02-contract-deployment.md)
- Understanding of cross-chain protocols and bridges
- Experience with multi-chain development
- Knowledge of cryptographic primitives and security

---

## 1. Bridge Architecture Overview

### Supported Networks and Assets

```yaml
# bridge-config.yml
supported_networks:
  ethereum:
    chain_id: 1
    name: 'Ethereum Mainnet'
    rpc_url: 'https://mainnet.infura.io/v3/YOUR-PROJECT-ID'
    bridge_contract: '0x742d35Cc6C4C73006cF3D741eE919dDD13F7f0C1'
    supported_assets:
      - WRF: '0x123...789' # Wrapped WRF on Ethereum
      - USDC: '0xA0b86a33E6F75ED1211B30a2a25f7AE3BF43C0D2'
      - WETH: '0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2'
    gas_limits:
      deposit: 150000
      withdraw: 200000

  polygon:
    chain_id: 137
    name: 'Polygon Mainnet'
    rpc_url: 'https://polygon-rpc.com'
    bridge_contract: '0x456d35Cc6C4C73006cF3D741eE919dDD13F7f0C2'
    supported_assets:
      - WRF: '0x456...123' # Wrapped WRF on Polygon
      - USDC: '0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174'
      - WMATIC: '0x0d500B1d8E8eF31E21C99d1Db9A6444d3ADf1270'
    gas_limits:
      deposit: 100000
      withdraw: 120000

  binance_smart_chain:
    chain_id: 56
    name: 'BNB Smart Chain'
    rpc_url: 'https://bsc-dataseed1.binance.org'
    bridge_contract: '0x789d35Cc6C4C73006cF3D741eE919dDD13F7f0C3'
    supported_assets:
      - WRF: '0x789...456' # Wrapped WRF on BSC
      - USDT: '0x55d398326f99059fF775485246999027B3197955'
      - WBNB: '0xbb4CdB9CBd36B01bD1cBaEBF2De08d9173bc095c'
    gas_limits:
      deposit: 80000
      withdraw: 100000

  arbitrum:
    chain_id: 42161
    name: 'Arbitrum One'
    rpc_url: 'https://arb1.arbitrum.io/rpc'
    bridge_contract: '0xABC...DEF'
    supported_assets:
      - WRF: '0xABC...DEF'
      - USDC: '0xA0b86a33E6F75ED1211B30a2a25f7AE3BF43C0D2'
    gas_limits:
      deposit: 200000
      withdraw: 250000

bridge_features:
  supported_operations:
    - asset_transfer
    - liquidity_provision
    - cross_chain_staking
    - governance_voting
    - nft_transfers

  security_mechanisms:
    - multi_signature_validation
    - time_delayed_withdrawals
    - fraud_proof_system
    - emergency_pause_mechanism
    - validator_slashing

  performance:
    - confirmation_time: '5-30 minutes'
    - throughput: '100-1000 TPS'
    - fees: 'dynamic based on network congestion'
```

### Bridge Protocol Architecture

```javascript
// architecture/bridge-protocol.js
class CrossChainBridgeProtocol {
  constructor(config) {
    this.config = config;
    this.validators = new Map();
    this.relayers = new Map();
    this.watchers = new Map();
    this.messageQueue = [];
    this.securityModule = new BridgeSecurityModule();
  }

  // Core bridge components
  components = {
    validator_network: {
      purpose: 'Validate cross-chain transactions',
      participants: 'Trusted validator nodes',
      consensus: 'Byzantine Fault Tolerant',
      threshold: '2/3 majority required',
      slashing: 'Economic penalties for misbehavior',
    },

    relayer_network: {
      purpose: 'Submit validated transactions to destination chain',
      participants: 'Incentivized relayer nodes',
      rewards: 'Transaction fees and bridge tokens',
      monitoring: 'Performance and reliability tracking',
    },

    watchers: {
      purpose: 'Monitor for fraudulent activities',
      detection: 'Automated fraud detection algorithms',
      response: 'Emergency pause and investigation',
      rewards: 'Bug bounty program',
    },

    liquidity_pools: {
      purpose: 'Provide instant liquidity for bridge transfers',
      providers: 'Liquidity providers earning fees',
      management: 'Automated market making algorithms',
      insurance: 'Slashing insurance for LP protection',
    },
  };

  async initialize() {
    console.log('🌉 Initializing cross-chain bridge protocol...');

    try {
      // Initialize validator network
      await this._initializeValidators();

      // Initialize relayer network
      await this._initializeRelayers();

      // Initialize watchers
      await this._initializeWatchers();

      // Initialize liquidity pools
      await this._initializeLiquidityPools();

      // Start monitoring services
      await this._startMonitoringServices();

      console.log('✅ Bridge protocol initialized successfully');
    } catch (error) {
      console.error('❌ Bridge initialization failed:', error);
      throw error;
    }
  }

  async _initializeValidators() {
    // Load validator configuration
    for (const validatorConfig of this.config.validators) {
      const validator = new BridgeValidator({
        address: validatorConfig.address,
        publicKey: validatorConfig.publicKey,
        stake: validatorConfig.stake,
        networks: validatorConfig.networks,
        reputation: validatorConfig.reputation || 100,
      });

      await validator.initialize();
      this.validators.set(validator.address, validator);

      console.log(`📝 Validator initialized: ${validator.address}`);
    }

    console.log(`✅ Initialized ${this.validators.size} validators`);
  }

  async _initializeRelayers() {
    for (const relayerConfig of this.config.relayers) {
      const relayer = new BridgeRelayer({
        address: relayerConfig.address,
        networks: relayerConfig.networks,
        gasPriceStrategy: relayerConfig.gasPriceStrategy,
        maxGasPrice: relayerConfig.maxGasPrice,
      });

      await relayer.initialize();
      this.relayers.set(relayer.address, relayer);

      console.log(`🚀 Relayer initialized: ${relayer.address}`);
    }

    console.log(`✅ Initialized ${this.relayers.size} relayers`);
  }

  async _initializeWatchers() {
    const watcherConfig = this.config.watchers;

    const fraudDetectionWatcher = new FraudDetectionWatcher({
      networks: Object.keys(this.config.networks),
      alertThresholds: watcherConfig.alertThresholds,
      notificationEndpoints: watcherConfig.notifications,
    });

    await fraudDetectionWatcher.initialize();
    this.watchers.set('fraud_detection', fraudDetectionWatcher);

    console.log('🔍 Fraud detection watcher initialized');
  }

  async _initializeLiquidityPools() {
    for (const [networkName, networkConfig] of Object.entries(this.config.networks)) {
      const liquidityPool = new BridgeLiquidityPool({
        network: networkName,
        contractAddress: networkConfig.liquidity_pool_contract,
        supportedAssets: networkConfig.supported_assets,
        feeStructure: networkConfig.fee_structure,
      });

      await liquidityPool.initialize();

      console.log(`💰 Liquidity pool initialized for ${networkName}`);
    }
  }

  async _startMonitoringServices() {
    // Start block monitoring for all networks
    for (const [networkName, networkConfig] of Object.entries(this.config.networks)) {
      this._startBlockMonitoring(networkName, networkConfig);
    }

    // Start validator performance monitoring
    this._startValidatorMonitoring();

    // Start relayer performance monitoring
    this._startRelayerMonitoring();

    console.log('📊 Monitoring services started');
  }

  _startBlockMonitoring(networkName, networkConfig) {
    const blockMonitor = new BlockMonitor({
      network: networkName,
      rpcUrl: networkConfig.rpc_url,
      bridgeContract: networkConfig.bridge_contract,
      startBlock: networkConfig.start_block || 'latest',
    });

    blockMonitor.on('deposit_event', event => {
      this._handleDepositEvent(networkName, event);
    });

    blockMonitor.on('withdrawal_event', event => {
      this._handleWithdrawalEvent(networkName, event);
    });

    blockMonitor.start();

    console.log(`👁️  Block monitoring started for ${networkName}`);
  }

  async _handleDepositEvent(sourceNetwork, event) {
    console.log(`📥 Deposit detected on ${sourceNetwork}:`, event.transactionHash);

    try {
      // Create cross-chain message
      const message = this._createCrossChainMessage(sourceNetwork, event);

      // Add to validation queue
      this.messageQueue.push(message);

      // Request validation from validator network
      await this._requestValidation(message);
    } catch (error) {
      console.error('❌ Deposit handling failed:', error);
    }
  }

  async _handleWithdrawalEvent(targetNetwork, event) {
    console.log(`📤 Withdrawal completed on ${targetNetwork}:`, event.transactionHash);

    // Update transaction status
    await this._updateTransactionStatus(event.messageId, 'completed', {
      targetTxHash: event.transactionHash,
      completedAt: new Date().toISOString(),
    });
  }

  _createCrossChainMessage(sourceNetwork, event) {
    return {
      id: this._generateMessageId(),
      sourceNetwork,
      targetNetwork: event.targetNetwork,
      sourceTransaction: event.transactionHash,
      sender: event.sender,
      recipient: event.recipient,
      asset: event.asset,
      amount: event.amount,
      nonce: event.nonce,
      timestamp: new Date().toISOString(),
      status: 'pending_validation',
      validations: [],
      relayAttempts: 0,
    };
  }

  async _requestValidation(message) {
    console.log(`🔍 Requesting validation for message ${message.id}`);

    // Send validation request to all validators
    const validationPromises = Array.from(this.validators.values()).map(validator =>
      validator.validateMessage(message)
    );

    try {
      const validationResults = await Promise.allSettled(validationPromises);

      // Process validation results
      this._processValidationResults(message, validationResults);
    } catch (error) {
      console.error('❌ Validation request failed:', error);
    }
  }

  _processValidationResults(message, results) {
    let approvals = 0;
    let rejections = 0;

    for (const result of results) {
      if (result.status === 'fulfilled' && result.value.approved) {
        approvals++;
        message.validations.push({
          validator: result.value.validator,
          approved: true,
          signature: result.value.signature,
          timestamp: new Date().toISOString(),
        });
      } else {
        rejections++;
        message.validations.push({
          validator: result.value?.validator || 'unknown',
          approved: false,
          reason: result.reason?.message || 'Validation failed',
          timestamp: new Date().toISOString(),
        });
      }
    }

    const totalValidators = this.validators.size;
    const threshold = Math.ceil((totalValidators * 2) / 3); // 2/3 majority

    if (approvals >= threshold) {
      message.status = 'validated';
      console.log(`✅ Message ${message.id} validated (${approvals}/${totalValidators})`);

      // Submit to relayer network
      this._submitToRelayers(message);
    } else {
      message.status = 'rejected';
      console.log(`❌ Message ${message.id} rejected (${approvals}/${totalValidators})`);
    }
  }

  async _submitToRelayers(message) {
    console.log(`🚀 Submitting message ${message.id} to relayers`);

    // Select best relayer for target network
    const targetRelayers = Array.from(this.relayers.values())
      .filter(relayer => relayer.supportsNetwork(message.targetNetwork))
      .sort((a, b) => b.reputation - a.reputation);

    if (targetRelayers.length === 0) {
      console.error(`❌ No relayers available for ${message.targetNetwork}`);
      return;
    }

    // Try relayers in order of reputation
    for (const relayer of targetRelayers) {
      try {
        const result = await relayer.relayMessage(message);

        if (result.success) {
          message.status = 'relayed';
          message.targetTransaction = result.transactionHash;
          console.log(`✅ Message ${message.id} relayed by ${relayer.address}`);
          break;
        }
      } catch (error) {
        console.warn(`⚠️  Relayer ${relayer.address} failed:`, error.message);
        continue;
      }
    }
  }

  _generateMessageId() {
    return crypto.randomBytes(32).toString('hex');
  }

  async _updateTransactionStatus(messageId, status, metadata = {}) {
    // Update transaction status in database/storage
    console.log(`📊 Transaction ${messageId} status updated: ${status}`);
  }

  // Public API methods
  async getBridgeStatus() {
    return {
      validators: {
        total: this.validators.size,
        active: Array.from(this.validators.values()).filter(v => v.isActive).length,
      },
      relayers: {
        total: this.relayers.size,
        active: Array.from(this.relayers.values()).filter(r => r.isActive).length,
      },
      messageQueue: this.messageQueue.length,
      networks: Object.keys(this.config.networks),
    };
  }

  async getTransactionStatus(messageId) {
    const message = this.messageQueue.find(m => m.id === messageId);
    if (!message) {
      throw new Error(`Transaction ${messageId} not found`);
    }

    return {
      id: message.id,
      status: message.status,
      sourceNetwork: message.sourceNetwork,
      targetNetwork: message.targetNetwork,
      sourceTransaction: message.sourceTransaction,
      targetTransaction: message.targetTransaction,
      validations: message.validations.length,
      timestamp: message.timestamp,
    };
  }
}

module.exports = CrossChainBridgeProtocol;
```

---

## 2. Bridge Client Implementation

### Cross-Chain Transaction Manager

```javascript
// client/cross-chain-client.js
const { ethers } = require('ethers');

class CrossChainClient {
  constructor(config) {
    this.config = config;
    this.providers = new Map();
    this.bridgeContracts = new Map();
    this.signers = new Map();
    this.transactionHistory = [];
  }

  async initialize() {
    console.log('🌉 Initializing cross-chain client...');

    try {
      // Initialize providers for each network
      for (const [networkName, networkConfig] of Object.entries(this.config.networks)) {
        await this._initializeNetwork(networkName, networkConfig);
      }

      console.log(`✅ Cross-chain client initialized for ${this.providers.size} networks`);
    } catch (error) {
      console.error('❌ Cross-chain client initialization failed:', error);
      throw error;
    }
  }

  async _initializeNetwork(networkName, networkConfig) {
    // Create provider
    const provider = new ethers.providers.JsonRpcProvider(networkConfig.rpc_url);
    this.providers.set(networkName, provider);

    // Create signer if private key provided
    if (this.config.privateKey) {
      const signer = new ethers.Wallet(this.config.privateKey, provider);
      this.signers.set(networkName, signer);
    }

    // Load bridge contract
    const bridgeContract = new ethers.Contract(
      networkConfig.bridge_contract,
      this._getBridgeABI(),
      this.signers.get(networkName) || provider
    );
    this.bridgeContracts.set(networkName, bridgeContract);

    console.log(`📡 Network initialized: ${networkName} (Chain ID: ${networkConfig.chain_id})`);
  }

  // Asset transfer methods
  async transferAsset(fromNetwork, toNetwork, asset, amount, recipient) {
    console.log(`🔄 Initiating cross-chain transfer:`);
    console.log(`   From: ${fromNetwork} → To: ${toNetwork}`);
    console.log(`   Asset: ${asset}, Amount: ${amount}`);
    console.log(`   Recipient: ${recipient}`);

    try {
      // Validate transfer parameters
      await this._validateTransfer(fromNetwork, toNetwork, asset, amount);

      // Estimate fees
      const fees = await this._estimateTransferFees(fromNetwork, toNetwork, asset, amount);
      console.log(`💰 Estimated fees: ${fees.totalFee} ${fees.feeAsset}`);

      // Execute deposit on source network
      const depositTx = await this._executeDeposit(
        fromNetwork,
        toNetwork,
        asset,
        amount,
        recipient,
        fees
      );
      console.log(`📥 Deposit transaction: ${depositTx.hash}`);

      // Wait for confirmation
      console.log('⏳ Waiting for deposit confirmation...');
      const receipt = await depositTx.wait();
      console.log(`✅ Deposit confirmed in block ${receipt.blockNumber}`);

      // Monitor cross-chain progress
      const transferId = this._extractTransferId(receipt);
      await this._monitorCrossChainTransfer(transferId, fromNetwork, toNetwork);

      return {
        transferId,
        depositTxHash: depositTx.hash,
        fromNetwork,
        toNetwork,
        asset,
        amount,
        recipient,
        status: 'confirmed',
        fees,
      };
    } catch (error) {
      console.error('❌ Cross-chain transfer failed:', error);
      throw error;
    }
  }

  async _validateTransfer(fromNetwork, toNetwork, asset, amount) {
    // Check if networks are supported
    if (!this.providers.has(fromNetwork)) {
      throw new Error(`Unsupported source network: ${fromNetwork}`);
    }
    if (!this.providers.has(toNetwork)) {
      throw new Error(`Unsupported target network: ${toNetwork}`);
    }

    // Check if asset is supported on both networks
    const fromNetworkConfig = this.config.networks[fromNetwork];
    const toNetworkConfig = this.config.networks[toNetwork];

    if (!fromNetworkConfig.supported_assets[asset]) {
      throw new Error(`Asset ${asset} not supported on ${fromNetwork}`);
    }
    if (!toNetworkConfig.supported_assets[asset]) {
      throw new Error(`Asset ${asset} not supported on ${toNetwork}`);
    }

    // Check minimum transfer amounts
    const minAmount = fromNetworkConfig.min_transfer_amounts?.[asset] || '0';
    if (ethers.utils.parseUnits(amount).lt(ethers.utils.parseUnits(minAmount))) {
      throw new Error(`Amount below minimum: ${minAmount} ${asset}`);
    }

    // Check user balance
    const userAddress = this.signers.get(fromNetwork)?.address;
    if (userAddress) {
      const balance = await this._getAssetBalance(fromNetwork, asset, userAddress);
      if (balance.lt(ethers.utils.parseUnits(amount))) {
        throw new Error(`Insufficient balance: ${ethers.utils.formatUnits(balance)} ${asset}`);
      }
    }

    console.log('✅ Transfer validation passed');
  }

  async _estimateTransferFees(fromNetwork, toNetwork, asset, amount) {
    const fromBridge = this.bridgeContracts.get(fromNetwork);
    const fromNetworkConfig = this.config.networks[fromNetwork];

    try {
      // Get bridge fee
      const bridgeFee = await fromBridge.calculateFee(
        this.config.networks[toNetwork].chain_id,
        fromNetworkConfig.supported_assets[asset],
        ethers.utils.parseUnits(amount)
      );

      // Estimate gas cost
      const gasPrice = await this.providers.get(fromNetwork).getGasPrice();
      const gasLimit = fromNetworkConfig.gas_limits.deposit;
      const gasCost = gasPrice.mul(gasLimit);

      return {
        bridgeFee: ethers.utils.formatUnits(bridgeFee),
        gasCost: ethers.utils.formatEther(gasCost),
        totalFee: ethers.utils.formatUnits(bridgeFee.add(gasCost)),
        feeAsset: fromNetwork === 'ethereum' ? 'ETH' : 'WRF',
      };
    } catch (error) {
      console.error('❌ Fee estimation failed:', error);
      throw error;
    }
  }

  async _executeDeposit(fromNetwork, toNetwork, asset, amount, recipient, fees) {
    const bridgeContract = this.bridgeContracts.get(fromNetwork);
    const fromNetworkConfig = this.config.networks[fromNetwork];
    const toNetworkConfig = this.config.networks[toNetwork];

    // Prepare transaction parameters
    const assetAddress = fromNetworkConfig.supported_assets[asset];
    const amountWei = ethers.utils.parseUnits(amount);
    const targetChainId = toNetworkConfig.chain_id;

    // Check if asset approval is needed (for ERC20 tokens)
    if (asset !== 'ETH' && asset !== 'WRF') {
      await this._ensureAssetApproval(fromNetwork, asset, amountWei);
    }

    try {
      // Execute deposit transaction
      const tx = await bridgeContract.deposit(targetChainId, assetAddress, amountWei, recipient, {
        value:
          asset === 'ETH'
            ? amountWei.add(ethers.utils.parseUnits(fees.bridgeFee))
            : ethers.utils.parseUnits(fees.bridgeFee),
        gasLimit: fromNetworkConfig.gas_limits.deposit,
      });

      return tx;
    } catch (error) {
      console.error('❌ Deposit execution failed:', error);
      throw error;
    }
  }

  async _ensureAssetApproval(network, asset, amount) {
    const networkConfig = this.config.networks[network];
    const assetAddress = networkConfig.supported_assets[asset];
    const bridgeAddress = networkConfig.bridge_contract;
    const signer = this.signers.get(network);

    // Create token contract instance
    const tokenContract = new ethers.Contract(
      assetAddress,
      [
        'function allowance(address owner, address spender) view returns (uint256)',
        'function approve(address spender, uint256 amount) returns (bool)',
      ],
      signer
    );

    // Check current allowance
    const currentAllowance = await tokenContract.allowance(signer.address, bridgeAddress);

    if (currentAllowance.lt(amount)) {
      console.log(`📝 Approving ${asset} for bridge contract...`);

      // Approve tokens
      const approveTx = await tokenContract.approve(bridgeAddress, amount);
      await approveTx.wait();

      console.log(`✅ ${asset} approval confirmed`);
    }
  }

  async _getAssetBalance(network, asset, address) {
    const networkConfig = this.config.networks[network];
    const provider = this.providers.get(network);

    if (asset === 'ETH' || asset === networkConfig.native_token) {
      return await provider.getBalance(address);
    } else {
      const assetAddress = networkConfig.supported_assets[asset];
      const tokenContract = new ethers.Contract(
        assetAddress,
        ['function balanceOf(address) view returns (uint256)'],
        provider
      );
      return await tokenContract.balanceOf(address);
    }
  }

  _extractTransferId(receipt) {
    // Extract transfer ID from deposit event logs
    const bridgeContract = this.bridgeContracts.get(receipt.to);
    const depositEvent = receipt.logs.find(log => {
      try {
        const parsed = bridgeContract.interface.parseLog(log);
        return parsed.name === 'Deposit';
      } catch {
        return false;
      }
    });

    if (depositEvent) {
      const parsed = bridgeContract.interface.parseLog(depositEvent);
      return parsed.args.transferId;
    }

    throw new Error('Transfer ID not found in transaction receipt');
  }

  async _monitorCrossChainTransfer(transferId, fromNetwork, toNetwork) {
    console.log(`👁️  Monitoring cross-chain transfer ${transferId}`);

    const maxWaitTime = 30 * 60 * 1000; // 30 minutes
    const pollInterval = 30 * 1000; // 30 seconds
    const startTime = Date.now();

    return new Promise((resolve, reject) => {
      const checkStatus = async () => {
        try {
          const status = await this._getTransferStatus(transferId, toNetwork);

          console.log(`📊 Transfer status: ${status.status}`);

          if (status.status === 'completed') {
            console.log(`✅ Cross-chain transfer completed: ${status.targetTxHash}`);
            resolve(status);
            return;
          }

          if (status.status === 'failed') {
            reject(new Error(`Cross-chain transfer failed: ${status.error}`));
            return;
          }

          // Check timeout
          if (Date.now() - startTime > maxWaitTime) {
            reject(new Error('Cross-chain transfer timeout'));
            return;
          }

          // Continue polling
          setTimeout(checkStatus, pollInterval);
        } catch (error) {
          console.error('❌ Status check failed:', error);
          setTimeout(checkStatus, pollInterval);
        }
      };

      // Start checking
      checkStatus();
    });
  }

  async _getTransferStatus(transferId, targetNetwork) {
    // Query bridge API or smart contract for transfer status
    // This would typically call the bridge protocol's status endpoint
    try {
      const response = await fetch(`${this.config.bridge_api}/status/${transferId}`);
      return await response.json();
    } catch (error) {
      // Fallback to contract query
      const targetBridge = this.bridgeContracts.get(targetNetwork);
      const transfer = await targetBridge.getTransfer(transferId);

      return {
        transferId,
        status: transfer.completed ? 'completed' : 'pending',
        targetTxHash: transfer.targetTxHash,
        timestamp: transfer.timestamp,
      };
    }
  }

  // Utility methods
  async getNetworkStatus(network) {
    const provider = this.providers.get(network);
    const bridgeContract = this.bridgeContracts.get(network);

    try {
      const [blockNumber, gasPrice, bridgeStatus] = await Promise.all([
        provider.getBlockNumber(),
        provider.getGasPrice(),
        bridgeContract.getStatus(),
      ]);

      return {
        network,
        blockNumber,
        gasPrice: ethers.utils.formatUnits(gasPrice, 'gwei'),
        bridgeStatus: {
          isActive: bridgeStatus.isActive,
          totalDeposits: bridgeStatus.totalDeposits.toString(),
          totalWithdrawals: bridgeStatus.totalWithdrawals.toString(),
        },
      };
    } catch (error) {
      console.error(`❌ Failed to get ${network} status:`, error);
      throw error;
    }
  }

  async getSupportedAssets(network) {
    const networkConfig = this.config.networks[network];
    return Object.keys(networkConfig.supported_assets);
  }

  async getTransferHistory(address) {
    // Query transfer history for address across all networks
    const history = [];

    for (const [networkName, networkConfig] of Object.entries(this.config.networks)) {
      try {
        const bridgeContract = this.bridgeContracts.get(networkName);
        const filter = bridgeContract.filters.Deposit(null, address);
        const events = await bridgeContract.queryFilter(filter, -10000); // Last 10k blocks

        for (const event of events) {
          history.push({
            transferId: event.args.transferId,
            fromNetwork: networkName,
            toNetwork: this._getNetworkByChainId(event.args.targetChainId),
            asset: event.args.asset,
            amount: ethers.utils.formatUnits(event.args.amount),
            recipient: event.args.recipient,
            timestamp: new Date(event.args.timestamp * 1000).toISOString(),
            txHash: event.transactionHash,
          });
        }
      } catch (error) {
        console.warn(`⚠️  Failed to get history from ${networkName}:`, error.message);
      }
    }

    return history.sort((a, b) => new Date(b.timestamp) - new Date(a.timestamp));
  }

  _getNetworkByChainId(chainId) {
    for (const [networkName, config] of Object.entries(this.config.networks)) {
      if (config.chain_id === chainId) {
        return networkName;
      }
    }
    return 'unknown';
  }

  _getBridgeABI() {
    // Simplified bridge contract ABI
    return [
      'function deposit(uint256 targetChainId, address asset, uint256 amount, address recipient) payable',
      'function withdraw(bytes32 transferId, bytes[] calldata signatures)',
      'function calculateFee(uint256 targetChainId, address asset, uint256 amount) view returns (uint256)',
      'function getTransfer(bytes32 transferId) view returns (tuple(bool completed, address recipient, uint256 amount, bytes32 targetTxHash, uint256 timestamp))',
      'function getStatus() view returns (tuple(bool isActive, uint256 totalDeposits, uint256 totalWithdrawals))',
      'event Deposit(bytes32 indexed transferId, address indexed sender, uint256 targetChainId, address asset, uint256 amount, address recipient, uint256 timestamp)',
      'event Withdrawal(bytes32 indexed transferId, address indexed recipient, uint256 amount, bytes32 targetTxHash)',
    ];
  }
}

module.exports = CrossChainClient;
```

---

## 3. Advanced Bridge Features

### Liquidity Pool Management

```javascript
// features/liquidity-pools.js
class BridgeLiquidityManager {
  constructor(bridgeClient) {
    this.bridgeClient = bridgeClient;
    this.liquidityPools = new Map();
    this.rebalancer = new AutoRebalancer();
    this.yieldOptimizer = new YieldOptimizer();
  }

  async initialize() {
    console.log('💰 Initializing liquidity pool manager...');

    try {
      // Initialize liquidity pools for each network
      for (const network of Object.keys(this.bridgeClient.config.networks)) {
        await this._initializeLiquidityPool(network);
      }

      // Start automated rebalancing
      await this._startRebalancing();

      // Start yield optimization
      await this._startYieldOptimization();

      console.log('✅ Liquidity pool manager initialized');
    } catch (error) {
      console.error('❌ Liquidity pool manager initialization failed:', error);
      throw error;
    }
  }

  async _initializeLiquidityPool(network) {
    const networkConfig = this.bridgeClient.config.networks[network];

    const liquidityPool = new LiquidityPool({
      network,
      contractAddress: networkConfig.liquidity_pool_contract,
      supportedAssets: networkConfig.supported_assets,
      provider: this.bridgeClient.providers.get(network),
      signer: this.bridgeClient.signers.get(network),
    });

    await liquidityPool.initialize();
    this.liquidityPools.set(network, liquidityPool);

    console.log(`💧 Liquidity pool initialized for ${network}`);
  }

  // Liquidity provider operations
  async addLiquidity(network, asset, amount, options = {}) {
    console.log(`➕ Adding liquidity: ${amount} ${asset} to ${network}`);

    try {
      const liquidityPool = this.liquidityPools.get(network);
      if (!liquidityPool) {
        throw new Error(`Liquidity pool not found for ${network}`);
      }

      // Calculate expected LP tokens
      const expectedLPTokens = await liquidityPool.calculateLPTokens(asset, amount);

      // Check slippage tolerance
      if (options.maxSlippage) {
        const slippage = await liquidityPool.calculateSlippage(asset, amount);
        if (slippage > options.maxSlippage) {
          throw new Error(`Slippage too high: ${slippage}% > ${options.maxSlippage}%`);
        }
      }

      // Execute liquidity addition
      const tx = await liquidityPool.addLiquidity(asset, amount, {
        minLPTokens: expectedLPTokens.mul(100 - (options.maxSlippage || 1)).div(100),
        deadline: Math.floor(Date.now() / 1000) + (options.deadline || 1800), // 30 minutes
      });

      console.log(`✅ Liquidity added: ${tx.hash}`);

      return {
        transactionHash: tx.hash,
        expectedLPTokens: ethers.utils.formatUnits(expectedLPTokens),
        network,
        asset,
        amount,
      };
    } catch (error) {
      console.error('❌ Add liquidity failed:', error);
      throw error;
    }
  }

  async removeLiquidity(network, asset, lpTokenAmount, options = {}) {
    console.log(`➖ Removing liquidity: ${lpTokenAmount} LP tokens from ${network}`);

    try {
      const liquidityPool = this.liquidityPools.get(network);

      // Calculate expected asset amount
      const expectedAssetAmount = await liquidityPool.calculateAssetAmount(asset, lpTokenAmount);

      // Execute liquidity removal
      const tx = await liquidityPool.removeLiquidity(asset, lpTokenAmount, {
        minAssetAmount: expectedAssetAmount.mul(100 - (options.maxSlippage || 1)).div(100),
        deadline: Math.floor(Date.now() / 1000) + (options.deadline || 1800),
      });

      console.log(`✅ Liquidity removed: ${tx.hash}`);

      return {
        transactionHash: tx.hash,
        expectedAssetAmount: ethers.utils.formatUnits(expectedAssetAmount),
        network,
        asset,
        lpTokenAmount,
      };
    } catch (error) {
      console.error('❌ Remove liquidity failed:', error);
      throw error;
    }
  }

  // Pool analytics and monitoring
  async getLiquidityPoolStats(network, asset) {
    const liquidityPool = this.liquidityPools.get(network);

    try {
      const stats = await liquidityPool.getPoolStats(asset);

      return {
        network,
        asset,
        totalLiquidity: ethers.utils.formatUnits(stats.totalLiquidity),
        totalLPTokens: ethers.utils.formatUnits(stats.totalLPTokens),
        utilizationRate: stats.utilizationRate,
        apr: stats.currentAPR,
        volume24h: ethers.utils.formatUnits(stats.volume24h),
        fees24h: ethers.utils.formatUnits(stats.fees24h),
        price: ethers.utils.formatUnits(stats.tokenPrice),
      };
    } catch (error) {
      console.error(`❌ Failed to get pool stats for ${network}:`, error);
      throw error;
    }
  }

  async getAllPoolStats() {
    const allStats = {};

    for (const [network, liquidityPool] of this.liquidityPools) {
      try {
        const networkConfig = this.bridgeClient.config.networks[network];
        const networkStats = {};

        for (const asset of Object.keys(networkConfig.supported_assets)) {
          networkStats[asset] = await this.getLiquidityPoolStats(network, asset);
        }

        allStats[network] = networkStats;
      } catch (error) {
        console.warn(`⚠️  Failed to get stats for ${network}:`, error.message);
      }
    }

    return allStats;
  }

  async _startRebalancing() {
    console.log('⚖️  Starting automated rebalancing...');

    // Rebalance every hour
    setInterval(
      async () => {
        try {
          await this._performRebalancing();
        } catch (error) {
          console.error('❌ Rebalancing failed:', error);
        }
      },
      60 * 60 * 1000
    );
  }

  async _performRebalancing() {
    console.log('⚖️  Performing liquidity rebalancing...');

    const allStats = await this.getAllPoolStats();
    const rebalanceActions = this.rebalancer.calculateRebalancing(allStats);

    for (const action of rebalanceActions) {
      try {
        if (action.type === 'transfer') {
          await this._transferLiquidity(
            action.fromNetwork,
            action.toNetwork,
            action.asset,
            action.amount
          );
        } else if (action.type === 'add') {
          await this._addLiquidityFromReserves(action.network, action.asset, action.amount);
        }

        console.log(`✅ Rebalancing action completed: ${action.type}`);
      } catch (error) {
        console.error(`❌ Rebalancing action failed:`, error);
      }
    }
  }

  async _transferLiquidity(fromNetwork, toNetwork, asset, amount) {
    // Transfer liquidity between networks using the bridge
    console.log(`🔄 Transferring ${amount} ${asset} from ${fromNetwork} to ${toNetwork}`);

    await this.bridgeClient.transferAsset(
      fromNetwork,
      toNetwork,
      asset,
      amount,
      this.liquidityPools.get(toNetwork).contractAddress
    );
  }

  async _startYieldOptimization() {
    console.log('📈 Starting yield optimization...');

    // Optimize yields every 4 hours
    setInterval(
      async () => {
        try {
          await this._optimizeYields();
        } catch (error) {
          console.error('❌ Yield optimization failed:', error);
        }
      },
      4 * 60 * 60 * 1000
    );
  }

  async _optimizeYields() {
    console.log('📈 Optimizing LP yields...');

    const allStats = await this.getAllPoolStats();
    const optimizations = this.yieldOptimizer.calculateOptimizations(allStats);

    for (const optimization of optimizations) {
      try {
        if (optimization.type === 'compound') {
          await this._compoundRewards(optimization.network, optimization.asset);
        } else if (optimization.type === 'reallocate') {
          await this._reallocateLiquidity(
            optimization.fromAsset,
            optimization.toAsset,
            optimization.network,
            optimization.amount
          );
        }

        console.log(`✅ Yield optimization completed: ${optimization.type}`);
      } catch (error) {
        console.error(`❌ Yield optimization failed:`, error);
      }
    }
  }

  async _compoundRewards(network, asset) {
    const liquidityPool = this.liquidityPools.get(network);

    // Claim rewards and reinvest
    const rewards = await liquidityPool.claimRewards(asset);
    if (rewards.gt(0)) {
      await liquidityPool.addLiquidity(asset, ethers.utils.formatUnits(rewards));
      console.log(`🔄 Compounded ${ethers.utils.formatUnits(rewards)} ${asset} rewards`);
    }
  }
}

module.exports = BridgeLiquidityManager;
```

This comprehensive cross-chain bridge integration guide provides everything needed to integrate with
WRFCoin's cross-chain infrastructure, including asset transfers, liquidity management, and advanced
bridge features for seamless multi-chain operations.

---

## Next Steps

1. **Complete [Learning Pathways](../pathways/)**
2. **Build [Production Applications](../applications/)**
3. **Deploy [Cross-Chain DeFi Protocols](./02-defi-protocols.md)**

---

## Troubleshooting

### Common Issues

**Bridge Transaction Stuck:**

- Check validator network status
- Verify sufficient liquidity on target network
- Monitor for network congestion

**High Transaction Fees:**

- Use gas price optimization
- Consider alternative networks
- Time transfers during low congestion

**Liquidity Imbalance:**

- Monitor rebalancing algorithms
- Adjust pool parameters
- Implement dynamic fee structures
