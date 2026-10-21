# API Documentation - WRFCoin Security Contracts

## SecureStakingRewardCalculator.sol

### Contract Overview

Provides secure calculation of staking rewards with comprehensive arithmetic protection mechanisms.

**Contract Address**: `TBD` (Set during deployment) **Solidity Version**: `^0.8.28` **License**: MIT

### Core Functions

#### `calculateStakingRewards(StakingParameters memory params)`

**Description**: Calculate staking rewards with comprehensive SafeMath protection

**Parameters**:

```solidity
struct StakingParameters {
    uint256 stakedAmount;           // Amount of tokens staked
    uint256 baseRewardRate;         // Base reward rate per second
    uint256 stakeDuration;          // Duration of stake in seconds
    uint256 qualityMultiplier;      // Quality-based multiplier (1e18 = 100%)
    uint256 vintageBonus;           // Vintage bonus in basis points
    uint256 sustainabilityBonus;   // Sustainability bonus in basis points
    uint256 globalMultiplier;       // Global reward multiplier (1e18 = 100%)
    uint256 minStakeDuration;       // Minimum stake duration for bonuses
    uint256 maxStakeDuration;       // Maximum stake duration for maximum bonuses
}
```

**Returns**:

```solidity
struct RewardCalculationResult {
    uint256 baseReward;             // Base reward before bonuses
    uint256 qualityBoostedReward;   // Reward after quality multiplier
    uint256 vintageBonus;           // Vintage bonus amount
    uint256 sustainabilityBonus;   // Sustainability bonus amount
    uint256 durationBonus;          // Duration-based bonus
    uint256 totalReward;            // Final total reward
    uint256 effectiveMultiplier;    // Effective combined multiplier
    bool hasOverflowRisk;           // Flag indicating overflow risk
}
```

**Access Control**: `onlyAuthorizedCalculator` **Security Features**:

- Comprehensive overflow protection
- Input validation
- Bounds checking
- Anti-manipulation safeguards

**Gas Estimate**: ~150,000 gas

**Example Usage**:

```solidity
StakingParameters memory params = StakingParameters({
    stakedAmount: 1000 * 1e18,      // 1000 tokens
    baseRewardRate: 1e15,           // 0.001 reward per second
    stakeDuration: 365 days,        // 1 year
    qualityMultiplier: 15 * 1e17,   // 150%
    vintageBonus: 1000,             // 10% in basis points
    sustainabilityBonus: 500,       // 5% in basis points
    globalMultiplier: 1e18,         // 100%
    minStakeDuration: 7 days,
    maxStakeDuration: 1095 days
});

RewardCalculationResult memory result = calculator.calculateStakingRewards(params);
```

#### `previewRewardCalculation(StakingParameters memory params)`

**Description**: Preview reward calculation without state changes (view function)

**Parameters**: Same as `calculateStakingRewards` **Returns**: Same as `calculateStakingRewards`
**Access Control**: Public view function **Gas Estimate**: ~100,000 gas

#### `validateCalculationSafety(StakingParameters memory params)`

**Description**: Validate calculation safety before performing operations

**Returns**:

```solidity
struct ValidationResult {
    bool isValid;                   // Whether calculation is valid
    string errorMessage;            // Error message if invalid
    uint256 maxSafeAmount;          // Maximum safe amount for calculations
    uint256 recommendedAmount;      // Recommended safe amount
}
```

**Access Control**: Public pure function

### Quality Oracle Integration

#### `getQualityMultiplierFromOracle(address carbonBasketToken)`

**Description**: Get quality tier and multiplier from oracle

**Parameters**:

- `carbonBasketToken`: Address of the carbon basket token

**Returns**:

- `qualityTier`: Quality tier (1-5)
- `multiplier`: Quality multiplier (1e18 = 100%)

**Access Control**: Public view function

### Administrative Functions

#### `setAuthorizedCalculator(address calculator, bool authorized)`

**Description**: Set authorized calculator **Access Control**: `STAKING_MANAGER_ROLE`

#### `updateQualityTier(uint256 tier, uint256 multiplier, uint256 minScore, uint256 maxScore, string memory tierName)`

**Description**: Update quality tier configuration **Access Control**: `STAKING_MANAGER_ROLE`

#### `updateVintageYearBonus(uint256 year, uint256 bonusBasisPoints)`

**Description**: Update vintage year bonus **Access Control**: `STAKING_MANAGER_ROLE`

#### `pauseCalculations()` / `resumeCalculations()`

**Description**: Emergency pause/resume calculations **Access Control**: `EMERGENCY_ROLE`

### Constants

```solidity
uint256 public constant PRECISION = 1e18;              // 18 decimal precision
uint256 public constant BASIS_POINTS = 10000;          // 100% in basis points
uint256 public constant MAX_REWARD_RATE = 1e30;        // Maximum reward rate
uint256 public constant MAX_MULTIPLIER = 5 * PRECISION; // Maximum 5x multiplier
uint256 public constant MAX_BONUS_BASIS_POINTS = 5000;  // Maximum 50% bonus
uint256 public constant MIN_STAKE_DURATION = 1 hours;   // Minimum stake duration
uint256 public constant MAX_STAKE_DURATION = 1095 days; // Maximum stake duration
```

### Events

```solidity
event RewardCalculated(
    address indexed user,
    address indexed stakingContract,
    uint256 stakedAmount,
    uint256 baseReward,
    uint256 totalReward,
    uint256 effectiveMultiplier
);

event ArithmeticSecurityEvent(
    string eventType,
    address indexed user,
    uint256 value1,
    uint256 value2,
    uint256 result,
    string description
);

event OverflowRiskDetected(
    address indexed user,
    string calculationType,
    uint256 attemptedValue,
    uint256 safeMaxValue
);
```

---

## SecureMultiSigWallet.sol

### Contract Overview

Enhanced multi-signature wallet with comprehensive security against bypass attempts.

**Contract Address**: `TBD` (Set during deployment) **Solidity Version**: `^0.8.28` **License**: MIT

### Core Functions

#### `submitTransaction(address to, uint256 value, bytes memory data, string memory description, TransactionType txType, uint256 deadline)`

**Description**: Submit a new transaction for multi-signature approval

**Parameters**:

- `to`: Recipient address
- `value`: ETH amount to send
- `data`: Transaction data
- `description`: Human-readable description
- `txType`: Transaction type enum
- `deadline`: Transaction expiration timestamp

**Returns**: `transactionId` (uint256)

**Access Control**: `onlyOwner` **Security Features**:

- Rate limiting
- Daily limits
- Input validation
- Emergency mode protection

**Gas Estimate**: ~200,000 gas

#### `confirmTransaction(uint256 transactionId, bytes memory signature)`

**Description**: Confirm a transaction with cryptographic signature verification

**Parameters**:

- `transactionId`: ID of transaction to confirm
- `signature`: EIP-712 signature

**Access Control**: `onlyOwner` **Security Features**:

- EIP-712 signature verification
- Anti-replay protection
- Signature rate limiting
- Duplicate confirmation prevention

**Gas Estimate**: ~150,000 gas

#### `executeTransaction(uint256 transactionId)`

**Description**: Execute a fully confirmed transaction

**Parameters**:

- `transactionId`: ID of transaction to execute

**Access Control**: `onlyOwner` **Security Features**:

- Final validation before execution
- Checks-effects-interactions pattern
- Comprehensive audit logging

**Gas Estimate**: Variable (depends on transaction)

### Owner Management

#### `addOwner(address newOwner, uint256 newRequiredSignatures, bytes[] memory signatures, uint256 deadline, uint256 nonce)`

**Description**: Add a new owner with multi-signature approval

**Parameters**:

- `newOwner`: Address of new owner
- `newRequiredSignatures`: New required signature threshold
- `signatures`: Array of approval signatures
- `deadline`: Operation deadline
- `nonce`: Unique operation nonce

**Access Control**: Multi-signature validation **Security Features**:

- EIP-712 signature verification
- Nonce-based replay protection
- Comprehensive validation

#### `removeOwner(address ownerToRemove, uint256 newRequiredSignatures, bytes[] memory signatures, uint256 deadline, uint256 nonce)`

**Description**: Remove an owner with multi-signature approval

**Parameters**: Similar to `addOwner` **Security Features**: Same as `addOwner`

### Emergency Functions

#### `activateEmergencyMode()`

**Description**: Activate emergency mode to pause all operations **Access Control**:
`EMERGENCY_ROLE`

#### `deactivateEmergencyMode(bytes[] memory signatures)`

**Description**: Deactivate emergency mode with multi-signature approval **Access Control**:
`EMERGENCY_ROLE` + multi-signature validation

#### `emergencyRecover(address to, uint256 amount, bytes memory data)`

**Description**: Emergency recovery function **Access Control**: `emergencyRecoveryAddress` only
**Requirements**: Emergency mode + 7-day cooling period

### Transaction Types

```solidity
enum TransactionType {
    TRANSFER,           // ETH transfer
    TOKEN_TRANSFER,     // ERC20 token transfer
    CONTRACT_CALL,      // General contract interaction
    OWNER_MANAGEMENT,   // Owner addition/removal
    EMERGENCY_ACTION    // Emergency operations
}
```

### Security Constants

```solidity
uint256 public constant MAX_OWNERS = 20;
uint256 public constant MIN_REQUIRED_SIGNATURES = 2;
uint256 public constant MAX_REQUIRED_SIGNATURES = 15;
uint256 public constant TRANSACTION_EXPIRY_TIME = 7 days;
uint256 public constant SIGNATURE_REPLAY_WINDOW = 1 hours;
uint256 public constant MAX_DAILY_TRANSACTION_LIMIT = 100;
```

### Events

```solidity
event TransactionSubmitted(
    uint256 indexed transactionId,
    address indexed creator,
    address indexed to,
    uint256 value,
    string description
);

event TransactionConfirmed(
    uint256 indexed transactionId,
    address indexed owner,
    uint256 confirmations
);

event TransactionExecuted(
    uint256 indexed transactionId,
    address indexed executor,
    bool success,
    bytes returnData
);

event SecurityAlert(
    string alertType,
    address indexed actor,
    uint256 indexed transactionId,
    bytes32 dataHash
);
```

### View Functions

#### `getTransaction(uint256 transactionId)`

**Description**: Get complete transaction details **Returns**: Transaction struct fields

#### `getOwners()`

**Description**: Get array of all owners **Returns**: `address[]`

#### `getConfirmationCount(uint256 transactionId)`

**Description**: Get confirmation count for transaction **Returns**: `uint256`

#### `isTransactionConfirmed(uint256 transactionId, address owner)`

**Description**: Check if transaction is confirmed by specific owner **Returns**: `bool`

#### `getAuditLog(uint256 index)`

**Description**: Get audit log entry **Returns**: Audit log struct fields

### Error Codes

Common revert messages and their meanings:

- `"SecureMultiSigWallet: Not an owner"` - Caller is not an authorized owner
- `"SecureMultiSigWallet: Invalid signature"` - EIP-712 signature verification failed
- `"SecureMultiSigWallet: Insufficient confirmations"` - Not enough signatures for execution
- `"SecureMultiSigWallet: Transaction expired"` - Transaction deadline passed
- `"SecureMultiSigWallet: Rate limit exceeded"` - Daily transaction limit reached
- `"SecureMultiSigWallet: Emergency mode active"` - Operations paused due to emergency

## Integration Examples

### Basic Staking Reward Calculation

```solidity
// 1. Set up parameters
StakingParameters memory params = StakingParameters({
    stakedAmount: userStakeAmount,
    baseRewardRate: farmConfig.baseRate,
    stakeDuration: block.timestamp - userStakeTime,
    qualityMultiplier: qualityOracle.getMultiplier(token),
    vintageBonus: getVintageBonus(token),
    sustainabilityBonus: getSustainabilityBonus(token),
    globalMultiplier: globalConfig.multiplier,
    minStakeDuration: 7 days,
    maxStakeDuration: 1095 days
});

// 2. Calculate rewards
RewardCalculationResult memory result = rewardCalculator.calculateStakingRewards(params);

// 3. Use results
uint256 totalReward = result.totalReward;
bool hasSafetyRisk = result.hasOverflowRisk;
```

### Multi-Sig Transaction Workflow

```solidity
// 1. Submit transaction
uint256 txId = wallet.submitTransaction(
    recipient,
    amount,
    data,
    "Transfer to treasury",
    TransactionType.TRANSFER,
    block.timestamp + 1 days
);

// 2. Owners confirm with signatures
bytes memory signature = _signTransaction(txId, ownerPrivateKey);
wallet.confirmTransaction(txId, signature);

// 3. Execute when threshold reached (automatic or manual)
// wallet.executeTransaction(txId); // Manual execution if needed
```

## Security Considerations

### Input Validation

- All functions perform comprehensive input validation
- Bounds checking prevents overflow/underflow
- Type validation ensures data integrity

### Access Control

- Role-based access control (RBAC) implementation
- Multi-signature requirements for critical operations
- Emergency controls for incident response

### Cryptographic Security

- EIP-712 structured data signing
- Anti-replay protection with nonces
- Signature verification with ECDSA recovery

### Economic Security

- Daily spending limits
- Rate limiting mechanisms
- Overflow protection for large values
- Economic bounds checking

---

_This API documentation provides comprehensive technical specifications for integrating with
WRFCoin's security contracts._
