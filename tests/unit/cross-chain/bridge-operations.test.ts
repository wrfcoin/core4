/**
 * Cross-Chain Bridge Operations Tests
 * Task 114.3: Tests for 13 supported chains, verification pipeline, fraud detection
 *
 * Comprehensive tests for cross-chain bridge operations including
 * multi-chain support, transaction verification, and security mechanisms
 * Target: 45 tests
 */

import {
  describe,
  it,
  expect,
  beforeEach,
  afterEach,
  jest,
} from '@jest/globals';

// Supported chains
enum ChainId {
  ETHEREUM = 1,
  POLYGON = 137,
  BSC = 56,
  AVALANCHE = 43114,
  ARBITRUM = 42161,
  OPTIMISM = 10,
  FANTOM = 250,
  COSMOS = 'cosmos-hub-4',
  OSMOSIS = 'osmosis-1',
  POLKADOT = 0,
  KUSAMA = 2,
  SOLANA = 'mainnet-beta',
  WRFCOIN = 'wrfcoin-mainnet',
}

enum BridgeStatus {
  PENDING = 'PENDING',
  CONFIRMING = 'CONFIRMING',
  VERIFIED = 'VERIFIED',
  EXECUTING = 'EXECUTING',
  COMPLETED = 'COMPLETED',
  FAILED = 'FAILED',
  FRAUD_DETECTED = 'FRAUD_DETECTED',
}

interface BridgeTransaction {
  id: string;
  sourceChain: ChainId;
  destinationChain: ChainId;
  sender: string;
  recipient: string;
  amount: bigint;
  tokenAddress?: string;
  sourceTxHash?: string;
  destinationTxHash?: string;
  status: BridgeStatus;
  confirmations: number;
  requiredConfirmations: number;
  validators: string[];
  validatorSignatures: Map<string, string>;
  createdAt: Date;
  updatedAt: Date;
  completedAt?: Date;
  fraudEvidence?: any;
}

interface ChainConfig {
  chainId: ChainId;
  name: string;
  rpcUrl: string;
  bridgeContractAddress: string;
  confirmationsRequired: number;
  blockTime: number; // in seconds
  isActive: boolean;
}

// Mock Bridge Service
class MockBridgeService {
  private transactions: Map<string, BridgeTransaction> = new Map();
  private validators: Set<string> = new Set([
    'validator1',
    'validator2',
    'validator3',
    'validator4',
    'validator5',
  ]);
  private requiredValidatorSignatures = 3;
  private txCounter = 0;

  private chainConfigs: Map<ChainId, ChainConfig> = new Map([
    [
      ChainId.ETHEREUM,
      {
        chainId: ChainId.ETHEREUM,
        name: 'Ethereum',
        rpcUrl: 'https://eth-rpc',
        bridgeContractAddress: '0xbridge1',
        confirmationsRequired: 12,
        blockTime: 12,
        isActive: true,
      },
    ],
    [
      ChainId.POLYGON,
      {
        chainId: ChainId.POLYGON,
        name: 'Polygon',
        rpcUrl: 'https://polygon-rpc',
        bridgeContractAddress: '0xbridge2',
        confirmationsRequired: 64,
        blockTime: 2,
        isActive: true,
      },
    ],
    [
      ChainId.BSC,
      {
        chainId: ChainId.BSC,
        name: 'BSC',
        rpcUrl: 'https://bsc-rpc',
        bridgeContractAddress: '0xbridge3',
        confirmationsRequired: 15,
        blockTime: 3,
        isActive: true,
      },
    ],
    [
      ChainId.AVALANCHE,
      {
        chainId: ChainId.AVALANCHE,
        name: 'Avalanche',
        rpcUrl: 'https://avax-rpc',
        bridgeContractAddress: '0xbridge4',
        confirmationsRequired: 2,
        blockTime: 2,
        isActive: true,
      },
    ],
    [
      ChainId.ARBITRUM,
      {
        chainId: ChainId.ARBITRUM,
        name: 'Arbitrum',
        rpcUrl: 'https://arb-rpc',
        bridgeContractAddress: '0xbridge5',
        confirmationsRequired: 1,
        blockTime: 0.25,
        isActive: true,
      },
    ],
    [
      ChainId.OPTIMISM,
      {
        chainId: ChainId.OPTIMISM,
        name: 'Optimism',
        rpcUrl: 'https://op-rpc',
        bridgeContractAddress: '0xbridge6',
        confirmationsRequired: 1,
        blockTime: 2,
        isActive: true,
      },
    ],
    [
      ChainId.FANTOM,
      {
        chainId: ChainId.FANTOM,
        name: 'Fantom',
        rpcUrl: 'https://ftm-rpc',
        bridgeContractAddress: '0xbridge7',
        confirmationsRequired: 5,
        blockTime: 1,
        isActive: true,
      },
    ],
    [
      ChainId.COSMOS,
      {
        chainId: ChainId.COSMOS,
        name: 'Cosmos Hub',
        rpcUrl: 'https://cosmos-rpc',
        bridgeContractAddress: 'cosmos1bridge',
        confirmationsRequired: 10,
        blockTime: 6,
        isActive: true,
      },
    ],
    [
      ChainId.OSMOSIS,
      {
        chainId: ChainId.OSMOSIS,
        name: 'Osmosis',
        rpcUrl: 'https://osmosis-rpc',
        bridgeContractAddress: 'osmo1bridge',
        confirmationsRequired: 10,
        blockTime: 6,
        isActive: true,
      },
    ],
    [
      ChainId.POLKADOT,
      {
        chainId: ChainId.POLKADOT,
        name: 'Polkadot',
        rpcUrl: 'https://dot-rpc',
        bridgeContractAddress: '0xbridge10',
        confirmationsRequired: 30,
        blockTime: 6,
        isActive: true,
      },
    ],
    [
      ChainId.KUSAMA,
      {
        chainId: ChainId.KUSAMA,
        name: 'Kusama',
        rpcUrl: 'https://ksm-rpc',
        bridgeContractAddress: '0xbridge11',
        confirmationsRequired: 20,
        blockTime: 6,
        isActive: true,
      },
    ],
    [
      ChainId.SOLANA,
      {
        chainId: ChainId.SOLANA,
        name: 'Solana',
        rpcUrl: 'https://sol-rpc',
        bridgeContractAddress: 'SolBridge123',
        confirmationsRequired: 32,
        blockTime: 0.4,
        isActive: true,
      },
    ],
    [
      ChainId.WRFCOIN,
      {
        chainId: ChainId.WRFCOIN,
        name: 'WRFCoin',
        rpcUrl: 'https://wrf-rpc',
        bridgeContractAddress: '0xwrfbridge',
        confirmationsRequired: 6,
        blockTime: 10,
        isActive: true,
      },
    ],
  ]);

  // Initiate a bridge transaction
  async initiateBridgeTransfer(
    sourceChain: ChainId,
    destinationChain: ChainId,
    sender: string,
    recipient: string,
    amount: bigint,
    tokenAddress?: string
  ): Promise<BridgeTransaction> {
    // Validate chains are supported
    const sourceConfig = this.chainConfigs.get(sourceChain);
    const destConfig = this.chainConfigs.get(destinationChain);

    if (!sourceConfig || !sourceConfig.isActive) {
      throw new Error(`Source chain ${sourceChain} is not supported`);
    }

    if (!destConfig || !destConfig.isActive) {
      throw new Error(`Destination chain ${destinationChain} is not supported`);
    }

    if (sourceChain === destinationChain) {
      throw new Error('Source and destination chains must be different');
    }

    if (amount <= 0n) {
      throw new Error('Amount must be positive');
    }

    const tx: BridgeTransaction = {
      id: `bridge_${++this.txCounter}`,
      sourceChain,
      destinationChain,
      sender,
      recipient,
      amount,
      tokenAddress,
      status: BridgeStatus.PENDING,
      confirmations: 0,
      requiredConfirmations: sourceConfig.confirmationsRequired,
      validators: [],
      validatorSignatures: new Map(),
      createdAt: new Date(),
      updatedAt: new Date(),
    };

    this.transactions.set(tx.id, tx);
    return tx;
  }

  // Submit source transaction hash
  async submitSourceTxHash(
    txId: string,
    sourceTxHash: string
  ): Promise<BridgeTransaction> {
    const tx = this.transactions.get(txId);
    if (!tx) {
      throw new Error('Transaction not found');
    }

    if (tx.status !== BridgeStatus.PENDING) {
      throw new Error('Transaction already processed');
    }

    tx.sourceTxHash = sourceTxHash;
    tx.status = BridgeStatus.CONFIRMING;
    tx.updatedAt = new Date();

    return tx;
  }

  // Update confirmations
  async updateConfirmations(
    txId: string,
    confirmations: number
  ): Promise<BridgeTransaction> {
    const tx = this.transactions.get(txId);
    if (!tx) {
      throw new Error('Transaction not found');
    }

    tx.confirmations = confirmations;
    tx.updatedAt = new Date();

    if (
      confirmations >= tx.requiredConfirmations &&
      tx.status === BridgeStatus.CONFIRMING
    ) {
      tx.status = BridgeStatus.VERIFIED;
    }

    return tx;
  }

  // Validator signs transaction
  async validatorSign(
    txId: string,
    validatorId: string,
    signature: string
  ): Promise<BridgeTransaction> {
    const tx = this.transactions.get(txId);
    if (!tx) {
      throw new Error('Transaction not found');
    }

    if (!this.validators.has(validatorId)) {
      throw new Error('Invalid validator');
    }

    if (tx.status !== BridgeStatus.VERIFIED) {
      throw new Error('Transaction must be verified before signing');
    }

    if (tx.validatorSignatures.has(validatorId)) {
      throw new Error('Validator already signed');
    }

    tx.validatorSignatures.set(validatorId, signature);
    tx.validators.push(validatorId);
    tx.updatedAt = new Date();

    // Check if enough signatures to execute
    if (tx.validatorSignatures.size >= this.requiredValidatorSignatures) {
      tx.status = BridgeStatus.EXECUTING;
    }

    return tx;
  }

  // Complete bridge transaction
  async completeBridge(
    txId: string,
    destinationTxHash: string
  ): Promise<BridgeTransaction> {
    const tx = this.transactions.get(txId);
    if (!tx) {
      throw new Error('Transaction not found');
    }

    if (tx.status !== BridgeStatus.EXECUTING) {
      throw new Error('Transaction not ready for completion');
    }

    tx.destinationTxHash = destinationTxHash;
    tx.status = BridgeStatus.COMPLETED;
    tx.completedAt = new Date();
    tx.updatedAt = new Date();

    return tx;
  }

  // Detect and report fraud
  async reportFraud(txId: string, evidence: any): Promise<BridgeTransaction> {
    const tx = this.transactions.get(txId);
    if (!tx) {
      throw new Error('Transaction not found');
    }

    tx.status = BridgeStatus.FRAUD_DETECTED;
    tx.fraudEvidence = evidence;
    tx.updatedAt = new Date();

    return tx;
  }

  // Verify transaction on source chain
  async verifySourceTransaction(txId: string): Promise<{
    isValid: boolean;
    confirmations: number;
    sender: string;
    amount: bigint;
  }> {
    const tx = this.transactions.get(txId);
    if (!tx || !tx.sourceTxHash) {
      throw new Error('Transaction or source hash not found');
    }

    // Simulate verification
    return {
      isValid: true,
      confirmations: tx.confirmations,
      sender: tx.sender,
      amount: tx.amount,
    };
  }

  // Check double-spend attempt
  async checkDoubleSpend(
    sourceChain: ChainId,
    sourceTxHash: string
  ): Promise<boolean> {
    for (const tx of this.transactions.values()) {
      if (
        tx.sourceChain === sourceChain &&
        tx.sourceTxHash === sourceTxHash &&
        tx.status !== BridgeStatus.FRAUD_DETECTED
      ) {
        return true; // Double spend detected
      }
    }
    return false;
  }

  // Get transaction
  getTransaction(txId: string): BridgeTransaction | undefined {
    return this.transactions.get(txId);
  }

  // Get chain config
  getChainConfig(chainId: ChainId): ChainConfig | undefined {
    return this.chainConfigs.get(chainId);
  }

  // Get all supported chains
  getSupportedChains(): ChainId[] {
    return Array.from(this.chainConfigs.keys());
  }

  // Disable chain
  disableChain(chainId: ChainId): void {
    const config = this.chainConfigs.get(chainId);
    if (config) {
      config.isActive = false;
    }
  }

  // Enable chain
  enableChain(chainId: ChainId): void {
    const config = this.chainConfigs.get(chainId);
    if (config) {
      config.isActive = true;
    }
  }
}

describe('Cross-Chain Bridge Operations', () => {
  let bridgeService: MockBridgeService;

  beforeEach(() => {
    bridgeService = new MockBridgeService();
  });

  describe('Chain Support', () => {
    it('should support 13 different chains', () => {
      const chains = bridgeService.getSupportedChains();
      expect(chains.length).toBe(13);
    });

    it('should have Ethereum chain config', () => {
      const config = bridgeService.getChainConfig(ChainId.ETHEREUM);
      expect(config).toBeDefined();
      expect(config?.name).toBe('Ethereum');
      expect(config?.confirmationsRequired).toBe(12);
    });

    it('should have Polygon chain config', () => {
      const config = bridgeService.getChainConfig(ChainId.POLYGON);
      expect(config).toBeDefined();
      expect(config?.name).toBe('Polygon');
      expect(config?.confirmationsRequired).toBe(64);
    });

    it('should have BSC chain config', () => {
      const config = bridgeService.getChainConfig(ChainId.BSC);
      expect(config?.name).toBe('BSC');
    });

    it('should have Avalanche chain config', () => {
      const config = bridgeService.getChainConfig(ChainId.AVALANCHE);
      expect(config?.name).toBe('Avalanche');
    });

    it('should have Arbitrum chain config', () => {
      const config = bridgeService.getChainConfig(ChainId.ARBITRUM);
      expect(config?.name).toBe('Arbitrum');
      expect(config?.confirmationsRequired).toBe(1);
    });

    it('should have Optimism chain config', () => {
      const config = bridgeService.getChainConfig(ChainId.OPTIMISM);
      expect(config?.name).toBe('Optimism');
    });

    it('should have Fantom chain config', () => {
      const config = bridgeService.getChainConfig(ChainId.FANTOM);
      expect(config?.name).toBe('Fantom');
    });

    it('should have Cosmos Hub chain config', () => {
      const config = bridgeService.getChainConfig(ChainId.COSMOS);
      expect(config?.name).toBe('Cosmos Hub');
    });

    it('should have Osmosis chain config', () => {
      const config = bridgeService.getChainConfig(ChainId.OSMOSIS);
      expect(config?.name).toBe('Osmosis');
    });

    it('should have Polkadot chain config', () => {
      const config = bridgeService.getChainConfig(ChainId.POLKADOT);
      expect(config?.name).toBe('Polkadot');
    });

    it('should have Kusama chain config', () => {
      const config = bridgeService.getChainConfig(ChainId.KUSAMA);
      expect(config?.name).toBe('Kusama');
    });

    it('should have Solana chain config', () => {
      const config = bridgeService.getChainConfig(ChainId.SOLANA);
      expect(config?.name).toBe('Solana');
    });

    it('should have WRFCoin chain config', () => {
      const config = bridgeService.getChainConfig(ChainId.WRFCOIN);
      expect(config?.name).toBe('WRFCoin');
    });
  });

  describe('Bridge Transaction Initiation', () => {
    it('should initiate a bridge transfer from Ethereum to Polygon', async () => {
      const tx = await bridgeService.initiateBridgeTransfer(
        ChainId.ETHEREUM,
        ChainId.POLYGON,
        '0xsender123',
        '0xrecipient456',
        1000000000000000000n // 1 token
      );

      expect(tx.id).toBeDefined();
      expect(tx.status).toBe(BridgeStatus.PENDING);
      expect(tx.sourceChain).toBe(ChainId.ETHEREUM);
      expect(tx.destinationChain).toBe(ChainId.POLYGON);
    });

    it('should initiate a bridge transfer from BSC to Avalanche', async () => {
      const tx = await bridgeService.initiateBridgeTransfer(
        ChainId.BSC,
        ChainId.AVALANCHE,
        '0xsender',
        '0xrecipient',
        5000000000000000000n
      );

      expect(tx.sourceChain).toBe(ChainId.BSC);
      expect(tx.destinationChain).toBe(ChainId.AVALANCHE);
    });

    it('should initiate a bridge transfer from Cosmos to Osmosis', async () => {
      const tx = await bridgeService.initiateBridgeTransfer(
        ChainId.COSMOS,
        ChainId.OSMOSIS,
        'cosmos1sender',
        'osmo1recipient',
        1000000n
      );

      expect(tx.sourceChain).toBe(ChainId.COSMOS);
      expect(tx.destinationChain).toBe(ChainId.OSMOSIS);
    });

    it('should reject transfer to same chain', async () => {
      await expect(
        bridgeService.initiateBridgeTransfer(
          ChainId.ETHEREUM,
          ChainId.ETHEREUM,
          '0xsender',
          '0xrecipient',
          1000000000000000000n
        )
      ).rejects.toThrow('Source and destination chains must be different');
    });

    it('should reject transfer with zero amount', async () => {
      await expect(
        bridgeService.initiateBridgeTransfer(
          ChainId.ETHEREUM,
          ChainId.POLYGON,
          '0xsender',
          '0xrecipient',
          0n
        )
      ).rejects.toThrow('Amount must be positive');
    });

    it('should reject transfer with negative amount', async () => {
      await expect(
        bridgeService.initiateBridgeTransfer(
          ChainId.ETHEREUM,
          ChainId.POLYGON,
          '0xsender',
          '0xrecipient',
          -100n
        )
      ).rejects.toThrow('Amount must be positive');
    });

    it('should reject transfer from disabled chain', async () => {
      bridgeService.disableChain(ChainId.FANTOM);

      await expect(
        bridgeService.initiateBridgeTransfer(
          ChainId.FANTOM,
          ChainId.ETHEREUM,
          '0xsender',
          '0xrecipient',
          1000000000000000000n
        )
      ).rejects.toThrow('not supported');
    });

    it('should reject transfer to disabled chain', async () => {
      bridgeService.disableChain(ChainId.SOLANA);

      await expect(
        bridgeService.initiateBridgeTransfer(
          ChainId.ETHEREUM,
          ChainId.SOLANA,
          '0xsender',
          '0xrecipient',
          1000000000000000000n
        )
      ).rejects.toThrow('not supported');
    });
  });

  describe('Verification Pipeline', () => {
    it('should transition from PENDING to CONFIRMING when source tx hash submitted', async () => {
      const tx = await bridgeService.initiateBridgeTransfer(
        ChainId.ETHEREUM,
        ChainId.POLYGON,
        '0xsender',
        '0xrecipient',
        1000000000000000000n
      );

      const updated = await bridgeService.submitSourceTxHash(
        tx.id,
        '0xtxhash123'
      );

      expect(updated.status).toBe(BridgeStatus.CONFIRMING);
      expect(updated.sourceTxHash).toBe('0xtxhash123');
    });

    it('should transition to VERIFIED when enough confirmations', async () => {
      const tx = await bridgeService.initiateBridgeTransfer(
        ChainId.ETHEREUM,
        ChainId.POLYGON,
        '0xsender',
        '0xrecipient',
        1000000000000000000n
      );

      await bridgeService.submitSourceTxHash(tx.id, '0xtxhash');

      // Ethereum requires 12 confirmations
      const updated = await bridgeService.updateConfirmations(tx.id, 12);

      expect(updated.status).toBe(BridgeStatus.VERIFIED);
    });

    it('should remain CONFIRMING when not enough confirmations', async () => {
      const tx = await bridgeService.initiateBridgeTransfer(
        ChainId.ETHEREUM,
        ChainId.POLYGON,
        '0xsender',
        '0xrecipient',
        1000000000000000000n
      );

      await bridgeService.submitSourceTxHash(tx.id, '0xtxhash');

      // Only 5 confirmations, need 12
      const updated = await bridgeService.updateConfirmations(tx.id, 5);

      expect(updated.status).toBe(BridgeStatus.CONFIRMING);
    });

    it('should verify source transaction details', async () => {
      const tx = await bridgeService.initiateBridgeTransfer(
        ChainId.ETHEREUM,
        ChainId.POLYGON,
        '0xsender',
        '0xrecipient',
        1000000000000000000n
      );

      await bridgeService.submitSourceTxHash(tx.id, '0xtxhash');

      const verification = await bridgeService.verifySourceTransaction(tx.id);

      expect(verification.isValid).toBe(true);
      expect(verification.sender).toBe('0xsender');
      expect(verification.amount).toBe(1000000000000000000n);
    });
  });

  describe('Validator Consensus', () => {
    it('should accept validator signature', async () => {
      const tx = await bridgeService.initiateBridgeTransfer(
        ChainId.ETHEREUM,
        ChainId.POLYGON,
        '0xsender',
        '0xrecipient',
        1000000000000000000n
      );

      await bridgeService.submitSourceTxHash(tx.id, '0xtxhash');
      await bridgeService.updateConfirmations(tx.id, 12);

      const updated = await bridgeService.validatorSign(
        tx.id,
        'validator1',
        'sig1'
      );

      expect(updated.validators).toContain('validator1');
      expect(updated.validatorSignatures.get('validator1')).toBe('sig1');
    });

    it('should transition to EXECUTING with enough validator signatures', async () => {
      const tx = await bridgeService.initiateBridgeTransfer(
        ChainId.ETHEREUM,
        ChainId.POLYGON,
        '0xsender',
        '0xrecipient',
        1000000000000000000n
      );

      await bridgeService.submitSourceTxHash(tx.id, '0xtxhash');
      await bridgeService.updateConfirmations(tx.id, 12);

      await bridgeService.validatorSign(tx.id, 'validator1', 'sig1');
      await bridgeService.validatorSign(tx.id, 'validator2', 'sig2');
      const updated = await bridgeService.validatorSign(
        tx.id,
        'validator3',
        'sig3'
      );

      expect(updated.status).toBe(BridgeStatus.EXECUTING);
    });

    it('should reject invalid validator', async () => {
      const tx = await bridgeService.initiateBridgeTransfer(
        ChainId.ETHEREUM,
        ChainId.POLYGON,
        '0xsender',
        '0xrecipient',
        1000000000000000000n
      );

      await bridgeService.submitSourceTxHash(tx.id, '0xtxhash');
      await bridgeService.updateConfirmations(tx.id, 12);

      await expect(
        bridgeService.validatorSign(tx.id, 'fake_validator', 'sig')
      ).rejects.toThrow('Invalid validator');
    });

    it('should reject duplicate validator signature', async () => {
      const tx = await bridgeService.initiateBridgeTransfer(
        ChainId.ETHEREUM,
        ChainId.POLYGON,
        '0xsender',
        '0xrecipient',
        1000000000000000000n
      );

      await bridgeService.submitSourceTxHash(tx.id, '0xtxhash');
      await bridgeService.updateConfirmations(tx.id, 12);
      await bridgeService.validatorSign(tx.id, 'validator1', 'sig1');

      await expect(
        bridgeService.validatorSign(tx.id, 'validator1', 'sig2')
      ).rejects.toThrow('Validator already signed');
    });

    it('should reject signing before verification', async () => {
      const tx = await bridgeService.initiateBridgeTransfer(
        ChainId.ETHEREUM,
        ChainId.POLYGON,
        '0xsender',
        '0xrecipient',
        1000000000000000000n
      );

      await expect(
        bridgeService.validatorSign(tx.id, 'validator1', 'sig1')
      ).rejects.toThrow('must be verified');
    });
  });

  describe('Bridge Completion', () => {
    it('should complete bridge transaction', async () => {
      const tx = await bridgeService.initiateBridgeTransfer(
        ChainId.ETHEREUM,
        ChainId.POLYGON,
        '0xsender',
        '0xrecipient',
        1000000000000000000n
      );

      await bridgeService.submitSourceTxHash(tx.id, '0xsourcehash');
      await bridgeService.updateConfirmations(tx.id, 12);
      await bridgeService.validatorSign(tx.id, 'validator1', 'sig1');
      await bridgeService.validatorSign(tx.id, 'validator2', 'sig2');
      await bridgeService.validatorSign(tx.id, 'validator3', 'sig3');

      const completed = await bridgeService.completeBridge(tx.id, '0xdesthash');

      expect(completed.status).toBe(BridgeStatus.COMPLETED);
      expect(completed.destinationTxHash).toBe('0xdesthash');
      expect(completed.completedAt).toBeDefined();
    });

    it('should reject completion before execution', async () => {
      const tx = await bridgeService.initiateBridgeTransfer(
        ChainId.ETHEREUM,
        ChainId.POLYGON,
        '0xsender',
        '0xrecipient',
        1000000000000000000n
      );

      await expect(
        bridgeService.completeBridge(tx.id, '0xdesthash')
      ).rejects.toThrow('not ready for completion');
    });
  });

  describe('Fraud Detection', () => {
    it('should detect double-spend attempt', async () => {
      const tx1 = await bridgeService.initiateBridgeTransfer(
        ChainId.ETHEREUM,
        ChainId.POLYGON,
        '0xsender',
        '0xrecipient',
        1000000000000000000n
      );

      await bridgeService.submitSourceTxHash(tx1.id, '0xsamehash');

      const isDoubleSpend = await bridgeService.checkDoubleSpend(
        ChainId.ETHEREUM,
        '0xsamehash'
      );
      expect(isDoubleSpend).toBe(true);
    });

    it('should report fraud and update transaction status', async () => {
      const tx = await bridgeService.initiateBridgeTransfer(
        ChainId.ETHEREUM,
        ChainId.POLYGON,
        '0xsender',
        '0xrecipient',
        1000000000000000000n
      );

      const evidence = {
        type: 'double_spend',
        duplicateTxHash: '0xfraud',
        detectedAt: new Date(),
      };

      const updated = await bridgeService.reportFraud(tx.id, evidence);

      expect(updated.status).toBe(BridgeStatus.FRAUD_DETECTED);
      expect(updated.fraudEvidence).toBeDefined();
      expect(updated.fraudEvidence.type).toBe('double_spend');
    });
  });

  describe('Chain Management', () => {
    it('should disable a chain', () => {
      bridgeService.disableChain(ChainId.FANTOM);

      const config = bridgeService.getChainConfig(ChainId.FANTOM);
      expect(config?.isActive).toBe(false);
    });

    it('should enable a previously disabled chain', () => {
      bridgeService.disableChain(ChainId.FANTOM);
      bridgeService.enableChain(ChainId.FANTOM);

      const config = bridgeService.getChainConfig(ChainId.FANTOM);
      expect(config?.isActive).toBe(true);
    });
  });

  describe('Cross-Chain Scenarios', () => {
    it('should handle EVM to Cosmos bridge', async () => {
      const tx = await bridgeService.initiateBridgeTransfer(
        ChainId.ETHEREUM,
        ChainId.COSMOS,
        '0xevmsender',
        'cosmos1recipient',
        1000000000000000000n
      );

      expect(tx.sourceChain).toBe(ChainId.ETHEREUM);
      expect(tx.destinationChain).toBe(ChainId.COSMOS);
    });

    it('should handle Polkadot to Kusama bridge', async () => {
      const tx = await bridgeService.initiateBridgeTransfer(
        ChainId.POLKADOT,
        ChainId.KUSAMA,
        '5Dotsender',
        '5Ksmrecipient',
        1000000n
      );

      expect(tx.sourceChain).toBe(ChainId.POLKADOT);
      expect(tx.destinationChain).toBe(ChainId.KUSAMA);
    });

    it('should handle WRFCoin to Ethereum bridge', async () => {
      const tx = await bridgeService.initiateBridgeTransfer(
        ChainId.WRFCOIN,
        ChainId.ETHEREUM,
        'wrf1sender',
        '0xethrecipient',
        5000000000000000000n
      );

      expect(tx.sourceChain).toBe(ChainId.WRFCOIN);
      expect(tx.destinationChain).toBe(ChainId.ETHEREUM);
    });
  });
});
