# Integration Guide - WRFCoin Security Contracts

## Overview

This guide provides step-by-step instructions for integrating with the SecureStakingRewardCalculator
and SecureMultiSigWallet contracts in the WRFCoin platform.

## Prerequisites

### Development Environment

- Solidity ^0.8.28
- Hardhat development framework
- OpenZeppelin Contracts v5.0+
- Node.js 18+ with TypeScript

### Required Dependencies

```json
{
  "dependencies": {
    "@openzeppelin/contracts": "^5.0.0",
    "@openzeppelin/contracts-upgradeable": "^5.0.0",
    "ethers": "^6.0.0",
    "hardhat": "^2.19.0"
  }
}
```

### Network Configuration

```javascript
// hardhat.config.js
module.exports = {
  networks: {
    wrfcoin_mainnet: {
      url: 'https://rpc.wrfcoin.network',
      accounts: [process.env.PRIVATE_KEY],
      chainId: 2024,
      gasPrice: 20000000000,
    },
    wrfcoin_testnet: {
      url: 'https://testnet-rpc.wrfcoin.network',
      accounts: [process.env.TESTNET_PRIVATE_KEY],
      chainId: 2025,
      gasPrice: 10000000000,
    },
  },
};
```

## Contract Addresses

### Mainnet

- **SecureStakingRewardCalculator**: `0x...` (TBD)
- **SecureMultiSigWallet**: `0x...` (TBD)
- **QualityOracle**: `0x...` (TBD)

### Testnet

- **SecureStakingRewardCalculator**: `0x...` (TBD)
- **SecureMultiSigWallet**: `0x...` (TBD)
- **QualityOracle**: `0x...` (TBD)

## Integration Patterns

### 1. Staking Reward Integration

#### Basic Integration

```solidity
// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import "./interfaces/ISecureStakingRewardCalculator.sol";

contract YieldFarmingManager {
    ISecureStakingRewardCalculator public immutable rewardCalculator;

    constructor(address _rewardCalculator) {
        rewardCalculator = ISecureStakingRewardCalculator(_rewardCalculator);
    }

    function claimRewards(address user, address farmAddress) external {
        // Get user staking data
        UserStakeData memory userData = userStakes[user][farmAddress];
        FarmConfig memory farm = farms[farmAddress];

        // Prepare calculation parameters
        ISecureStakingRewardCalculator.StakingParameters memory params =
            ISecureStakingRewardCalculator.StakingParameters({
                stakedAmount: userData.amount,
                baseRewardRate: farm.baseRewardRate,
                stakeDuration: block.timestamp - userData.stakeTime,
                qualityMultiplier: farm.qualityMultiplier,
                vintageBonus: farm.vintageBonus,
                sustainabilityBonus: farm.sustainabilityBonus,
                globalMultiplier: globalMultiplier,
                minStakeDuration: 7 days,
                maxStakeDuration: 1095 days
            });

        // Calculate rewards securely
        ISecureStakingRewardCalculator.RewardCalculationResult memory result =
            rewardCalculator.calculateStakingRewards(params);

        // Validate calculation results
        require(!result.hasOverflowRisk, "YieldFarmingManager: Reward calculation overflow risk");
        require(result.totalReward > 0, "YieldFarmingManager: No rewards to claim");

        // Execute reward distribution
        _distributeRewards(user, result.totalReward);

        // Update user stake data
        userData.lastClaimTime = block.timestamp;
        userData.totalRewardsClaimed += result.totalReward;

        emit RewardsClaimed(user, farmAddress, result.totalReward, result.effectiveMultiplier);
    }
}
```

#### Advanced Integration with Quality Oracle

```solidity
contract AdvancedStakingManager {
    ISecureStakingRewardCalculator public immutable rewardCalculator;
    IQualityOracle public immutable qualityOracle;

    struct EnhancedStakeInfo {
        uint256 amount;
        uint256 stakeTime;
        address carbonBasketToken;
        uint256 qualityTier;
        uint256 lastUpdateTime;
        bool hasDynamicQuality;
    }

    mapping(address => mapping(address => EnhancedStakeInfo)) public userStakes;

    function calculateDynamicRewards(address user, address farmAddress)
        external
        view
        returns (ISecureStakingRewardCalculator.RewardCalculationResult memory)
    {
        EnhancedStakeInfo memory stakeInfo = userStakes[user][farmAddress];

        // Get latest quality data from oracle
        (uint256 qualityTier, uint256 qualityMultiplier) =
            rewardCalculator.getQualityMultiplierFromOracle(stakeInfo.carbonBasketToken);

        // Get vintage bonus for the specific token
        uint256 vintageYear = ICarbonBasketToken(stakeInfo.carbonBasketToken).getVintageYear();
        uint256 vintageBonus = rewardCalculator.getVintageYearBonus(vintageYear);

        // Prepare parameters with dynamic quality
        ISecureStakingRewardCalculator.StakingParameters memory params =
            ISecureStakingRewardCalculator.StakingParameters({
                stakedAmount: stakeInfo.amount,
                baseRewardRate: farms[farmAddress].baseRewardRate,
                stakeDuration: block.timestamp - stakeInfo.stakeTime,
                qualityMultiplier: qualityMultiplier,
                vintageBonus: vintageBonus,
                sustainabilityBonus: _calculateSustainabilityBonus(stakeInfo.carbonBasketToken),
                globalMultiplier: globalMultiplier,
                minStakeDuration: 7 days,
                maxStakeDuration: 1095 days
            });

        // Preview calculation (no state changes)
        return rewardCalculator.previewRewardCalculation(params);
    }
}
```

### 2. Multi-Signature Wallet Integration

#### Basic Treasury Integration

```solidity
contract TreasuryManager {
    ISecureMultiSigWallet public immutable treasury;

    event TreasuryTransactionSubmitted(uint256 indexed txId, address indexed recipient, uint256 amount);

    constructor(address _treasury) {
        treasury = ISecureMultiSigWallet(_treasury);
    }

    function proposeTreasuryTransfer(
        address recipient,
        uint256 amount,
        string memory description
    ) external returns (uint256 txId) {
        // Validate proposal
        require(recipient != address(0), "TreasuryManager: Invalid recipient");
        require(amount > 0, "TreasuryManager: Invalid amount");
        require(bytes(description).length > 0, "TreasuryManager: Description required");

        // Submit to multi-sig wallet
        txId = treasury.submitTransaction(
            recipient,
            amount,
            "",  // No data for simple transfer
            description,
            ISecureMultiSigWallet.TransactionType.TRANSFER,
            block.timestamp + 7 days  // 7-day deadline
        );

        emit TreasuryTransactionSubmitted(txId, recipient, amount);
        return txId;
    }

    function proposeTokenTransfer(
        address tokenContract,
        address recipient,
        uint256 amount,
        string memory description
    ) external returns (uint256 txId) {
        // Encode ERC20 transfer call
        bytes memory transferData = abi.encodeWithSelector(
            IERC20.transfer.selector,
            recipient,
            amount
        );

        // Submit to multi-sig wallet
        txId = treasury.submitTransaction(
            tokenContract,
            0,  // No ETH value for token transfer
            transferData,
            description,
            ISecureMultiSigWallet.TransactionType.TOKEN_TRANSFER,
            block.timestamp + 7 days
        );

        emit TreasuryTransactionSubmitted(txId, recipient, amount);
        return txId;
    }
}
```

#### Advanced Multi-Sig Integration with EIP-712

```solidity
contract GovernanceMultiSig {
    using ECDSA for bytes32;

    ISecureMultiSigWallet public immutable multiSigWallet;

    // EIP-712 domain separator
    bytes32 private immutable DOMAIN_SEPARATOR;

    // Typehash for governance proposals
    bytes32 private constant GOVERNANCE_PROPOSAL_TYPEHASH = keccak256(
        "GovernanceProposal(address target,uint256 value,bytes data,string description,uint256 deadline,uint256 nonce)"
    );

    constructor(address _multiSigWallet) {
        multiSigWallet = ISecureMultiSigWallet(_multiSigWallet);

        DOMAIN_SEPARATOR = keccak256(abi.encode(
            keccak256("EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)"),
            keccak256(bytes("WRFCoin Governance")),
            keccak256(bytes("1")),
            block.chainid,
            address(this)
        ));
    }

    function createSignedProposal(
        address target,
        uint256 value,
        bytes memory data,
        string memory description,
        uint256 deadline,
        uint256 nonce,
        bytes memory signature
    ) external returns (uint256 txId) {
        // Verify signature
        bytes32 structHash = keccak256(abi.encode(
            GOVERNANCE_PROPOSAL_TYPEHASH,
            target,
            value,
            keccak256(data),
            keccak256(bytes(description)),
            deadline,
            nonce
        ));

        bytes32 digest = keccak256(abi.encodePacked("\x19\x01", DOMAIN_SEPARATOR, structHash));
        address signer = digest.recover(signature);

        require(multiSigWallet.isOwner(signer), "GovernanceMultiSig: Invalid signer");

        // Submit to multi-sig wallet
        txId = multiSigWallet.submitTransaction(
            target,
            value,
            data,
            description,
            ISecureMultiSigWallet.TransactionType.CONTRACT_CALL,
            deadline
        );

        // Auto-confirm with the verified signature
        multiSigWallet.confirmTransaction(txId, signature);

        return txId;
    }
}
```

### 3. Emergency Response Integration

```solidity
contract EmergencyResponseManager {
    ISecureMultiSigWallet public immutable multiSigWallet;
    ISecureStakingRewardCalculator public immutable rewardCalculator;

    address public immutable emergencyCoordinator;
    mapping(address => bool) public authorizedResponders;

    event EmergencyActivated(address indexed activator, string reason);
    event EmergencyDeactivated(address indexed deactivator);
    event CalculationsPaused(address indexed pauser, string reason);

    modifier onlyEmergencyCoordinator() {
        require(msg.sender == emergencyCoordinator, "EmergencyResponseManager: Not emergency coordinator");
        _;
    }

    modifier onlyAuthorizedResponder() {
        require(authorizedResponders[msg.sender], "EmergencyResponseManager: Not authorized responder");
        _;
    }

    function activateEmergencyProtocol(string memory reason) external onlyAuthorizedResponder {
        // Activate emergency mode on multi-sig wallet
        multiSigWallet.activateEmergencyMode();

        // Pause calculations on reward calculator
        rewardCalculator.pauseCalculations();

        emit EmergencyActivated(msg.sender, reason);
        emit CalculationsPaused(msg.sender, reason);
    }

    function deactivateEmergencyProtocol(bytes[] memory signatures)
        external
        onlyEmergencyCoordinator
    {
        // Deactivate emergency mode (requires multi-sig approval)
        multiSigWallet.deactivateEmergencyMode(signatures);

        // Resume calculations
        rewardCalculator.resumeCalculations();

        emit EmergencyDeactivated(msg.sender);
    }
}
```

## TypeScript Integration

### Contract Interface Setup

```typescript
// contracts/interfaces.ts
import { ethers } from 'ethers';

export interface StakingParameters {
  stakedAmount: bigint;
  baseRewardRate: bigint;
  stakeDuration: bigint;
  qualityMultiplier: bigint;
  vintageBonus: bigint;
  sustainabilityBonus: bigint;
  globalMultiplier: bigint;
  minStakeDuration: bigint;
  maxStakeDuration: bigint;
}

export interface RewardCalculationResult {
  baseReward: bigint;
  qualityBoostedReward: bigint;
  vintageBonus: bigint;
  sustainabilityBonus: bigint;
  durationBonus: bigint;
  totalReward: bigint;
  effectiveMultiplier: bigint;
  hasOverflowRisk: boolean;
}

export enum TransactionType {
  TRANSFER = 0,
  TOKEN_TRANSFER = 1,
  CONTRACT_CALL = 2,
  OWNER_MANAGEMENT = 3,
  EMERGENCY_ACTION = 4,
}
```

### Service Layer Implementation

```typescript
// services/StakingService.ts
import { ethers } from 'ethers';
import { StakingParameters, RewardCalculationResult } from '../contracts/interfaces';

export class StakingService {
  private rewardCalculator: ethers.Contract;
  private provider: ethers.Provider;

  constructor(rewardCalculatorAddress: string, provider: ethers.Provider, signer?: ethers.Signer) {
    this.provider = provider;
    this.rewardCalculator = new ethers.Contract(
      rewardCalculatorAddress,
      REWARD_CALCULATOR_ABI,
      signer || provider
    );
  }

  async calculateRewards(params: StakingParameters): Promise<RewardCalculationResult> {
    try {
      // Validate parameters client-side first
      this.validateStakingParameters(params);

      // Call contract method
      const result = await this.rewardCalculator.calculateStakingRewards(params);

      return {
        baseReward: result.baseReward,
        qualityBoostedReward: result.qualityBoostedReward,
        vintageBonus: result.vintageBonus,
        sustainabilityBonus: result.sustainabilityBonus,
        durationBonus: result.durationBonus,
        totalReward: result.totalReward,
        effectiveMultiplier: result.effectiveMultiplier,
        hasOverflowRisk: result.hasOverflowRisk,
      };
    } catch (error) {
      console.error('Error calculating rewards:', error);
      throw new Error(`Reward calculation failed: ${error.message}`);
    }
  }

  async previewRewards(params: StakingParameters): Promise<RewardCalculationResult> {
    const result = await this.rewardCalculator.previewRewardCalculation(params);
    return this.parseRewardResult(result);
  }

  private validateStakingParameters(params: StakingParameters): void {
    if (params.stakedAmount <= 0n) {
      throw new Error('Staked amount must be greater than 0');
    }

    if (params.baseRewardRate <= 0n) {
      throw new Error('Base reward rate must be greater than 0');
    }

    if (params.stakeDuration < params.minStakeDuration) {
      throw new Error('Stake duration below minimum');
    }

    if (params.stakeDuration > params.maxStakeDuration) {
      throw new Error('Stake duration exceeds maximum');
    }
  }

  private parseRewardResult(result: any): RewardCalculationResult {
    return {
      baseReward: BigInt(result.baseReward.toString()),
      qualityBoostedReward: BigInt(result.qualityBoostedReward.toString()),
      vintageBonus: BigInt(result.vintageBonus.toString()),
      sustainabilityBonus: BigInt(result.sustainabilityBonus.toString()),
      durationBonus: BigInt(result.durationBonus.toString()),
      totalReward: BigInt(result.totalReward.toString()),
      effectiveMultiplier: BigInt(result.effectiveMultiplier.toString()),
      hasOverflowRisk: result.hasOverflowRisk,
    };
  }
}
```

### Multi-Sig Wallet Service

```typescript
// services/MultiSigService.ts
import { ethers } from 'ethers';
import { TransactionType } from '../contracts/interfaces';

export class MultiSigService {
  private wallet: ethers.Contract;
  private signer: ethers.Signer;

  constructor(walletAddress: string, signer: ethers.Signer) {
    this.signer = signer;
    this.wallet = new ethers.Contract(walletAddress, MULTISIG_WALLET_ABI, signer);
  }

  async submitTransaction(
    to: string,
    value: bigint,
    data: string,
    description: string,
    txType: TransactionType,
    deadline: number
  ): Promise<number> {
    const tx = await this.wallet.submitTransaction(to, value, data, description, txType, deadline);

    const receipt = await tx.wait();
    const event = receipt.logs.find(
      log => log.topics[0] === this.wallet.interface.getEventTopic('TransactionSubmitted')
    );

    if (!event) {
      throw new Error('Transaction submission failed');
    }

    const parsedEvent = this.wallet.interface.parseLog(event);
    return parsedEvent.args.transactionId.toNumber();
  }

  async confirmTransaction(txId: number): Promise<void> {
    // Generate EIP-712 signature
    const transaction = await this.wallet.getTransaction(txId);
    const signature = await this.generateTransactionSignature(transaction);

    const tx = await this.wallet.confirmTransaction(txId, signature);
    await tx.wait();
  }

  private async generateTransactionSignature(transaction: any): Promise<string> {
    const domain = {
      name: 'SecureMultiSigWallet',
      version: '1',
      chainId: await this.signer.getChainId(),
      verifyingContract: await this.wallet.getAddress(),
    };

    const types = {
      Transaction: [
        { name: 'to', type: 'address' },
        { name: 'value', type: 'uint256' },
        { name: 'data', type: 'bytes' },
        { name: 'nonce', type: 'uint256' },
        { name: 'deadline', type: 'uint256' },
        { name: 'description', type: 'string' },
      ],
    };

    const value = {
      to: transaction.to,
      value: transaction.value,
      data: transaction.data,
      nonce: transaction.nonce,
      deadline: transaction.expiresAt,
      description: transaction.description,
    };

    return await this.signer.signTypedData(domain, types, value);
  }
}
```

## Error Handling

### Common Error Scenarios

```typescript
// utils/ErrorHandler.ts
export class ContractErrorHandler {
  static parseContractError(error: any): string {
    if (error.reason) {
      return error.reason;
    }

    if (error.data && error.data.message) {
      return error.data.message;
    }

    // Parse custom error messages
    const errorMessage = error.message || error.toString();

    if (errorMessage.includes('SecureStakingRewardCalculator: ')) {
      return this.parseStakingError(errorMessage);
    }

    if (errorMessage.includes('SecureMultiSigWallet: ')) {
      return this.parseMultiSigError(errorMessage);
    }

    return 'Unknown contract error';
  }

  private static parseStakingError(message: string): string {
    if (message.includes('Multiplication overflow risk')) {
      return 'Staking amount too large for safe calculation. Please reduce the amount.';
    }

    if (message.includes('Not authorized calculator')) {
      return 'Unauthorized access to staking calculator. Contact administrator.';
    }

    if (message.includes('Calculations paused')) {
      return 'Staking calculations are temporarily paused for maintenance.';
    }

    return message;
  }

  private static parseMultiSigError(message: string): string {
    if (message.includes('Not an owner')) {
      return 'You are not authorized to perform this operation.';
    }

    if (message.includes('Invalid signature')) {
      return 'Transaction signature is invalid. Please try again.';
    }

    if (message.includes('Rate limit exceeded')) {
      return 'Transaction rate limit exceeded. Please wait before submitting another transaction.';
    }

    if (message.includes('Emergency mode active')) {
      return 'Wallet is in emergency mode. Operations are temporarily suspended.';
    }

    return message;
  }
}
```

## Testing Integration

### Unit Test Example

```typescript
// tests/integration/StakingIntegration.test.ts
import { expect } from 'chai';
import { ethers } from 'hardhat';
import { StakingService } from '../services/StakingService';

describe('Staking Integration Tests', () => {
  let stakingService: StakingService;
  let rewardCalculator: any;

  beforeEach(async () => {
    const [owner] = await ethers.getSigners();

    // Deploy contracts
    const RewardCalculator = await ethers.getContractFactory('SecureStakingRewardCalculator');
    rewardCalculator = await RewardCalculator.deploy(ethers.ZeroAddress); // Mock oracle

    stakingService = new StakingService(
      await rewardCalculator.getAddress(),
      ethers.provider,
      owner
    );
  });

  it('should calculate rewards correctly for valid parameters', async () => {
    const params = {
      stakedAmount: ethers.parseEther('1000'),
      baseRewardRate: ethers.parseUnits('0.001', 18),
      stakeDuration: BigInt(365 * 24 * 3600), // 1 year
      qualityMultiplier: ethers.parseEther('1.5'), // 150%
      vintageBonus: 1000n, // 10%
      sustainabilityBonus: 500n, // 5%
      globalMultiplier: ethers.parseEther('1'), // 100%
      minStakeDuration: BigInt(7 * 24 * 3600), // 7 days
      maxStakeDuration: BigInt(1095 * 24 * 3600), // 3 years
    };

    const result = await stakingService.previewRewards(params);

    expect(result.totalReward).to.be.gt(result.baseReward);
    expect(result.hasOverflowRisk).to.be.false;
    expect(result.effectiveMultiplier).to.be.gte(ethers.parseEther('1'));
  });

  it('should reject invalid parameters', async () => {
    const invalidParams = {
      stakedAmount: 0n, // Invalid: zero amount
      baseRewardRate: ethers.parseUnits('0.001', 18),
      stakeDuration: BigInt(365 * 24 * 3600),
      qualityMultiplier: ethers.parseEther('1'),
      vintageBonus: 0n,
      sustainabilityBonus: 0n,
      globalMultiplier: ethers.parseEther('1'),
      minStakeDuration: BigInt(7 * 24 * 3600),
      maxStakeDuration: BigInt(1095 * 24 * 3600),
    };

    await expect(stakingService.calculateRewards(invalidParams)).to.be.rejectedWith(
      'Staked amount must be greater than 0'
    );
  });
});
```

## Best Practices

### 1. Gas Optimization

- Use `previewRewardCalculation` for UI display before actual transactions
- Batch multiple operations when possible
- Cache frequently accessed data

### 2. Security Practices

- Always validate inputs client-side before contract calls
- Use proper error handling for all contract interactions
- Implement timeout mechanisms for long-running operations
- Monitor for emergency mode activations

### 3. User Experience

- Provide clear error messages for failed transactions
- Show transaction progress and confirmations
- Implement proper loading states
- Cache results to reduce network calls

### 4. Monitoring Integration

- Log all contract interactions for audit purposes
- Monitor gas usage and optimize accordingly
- Track error rates and response times
- Implement alerting for critical failures

---

_This integration guide provides comprehensive instructions for safely integrating with WRFCoin's
security contracts._
