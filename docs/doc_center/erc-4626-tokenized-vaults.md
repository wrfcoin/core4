# ERC-4626: Tokenized Vaults

## Overview

ERC-4626 is a standard for tokenized vault contracts that allows users to deposit tokens and receive
shares representing their ownership in the vault. This standard enables composable yield-bearing
positions and simplified DeFi integrations.

## Key Features

- **Standardized Interface**: Consistent API across all vault implementations
- **Share-based Accounting**: Proportional ownership through vault tokens
- **Yield Optimization**: Automated strategies for maximizing returns
- **Composability**: Easy integration with other DeFi protocols

## Core Functions

### Deposit Operations

```solidity
function deposit(uint256 assets, address receiver) external returns (uint256 shares);
function mint(uint256 shares, address receiver) external returns (uint256 assets);
```

### Withdrawal Operations

```solidity
function withdraw(uint256 assets, address receiver, address owner) external returns (uint256 shares);
function redeem(uint256 shares, address receiver, address owner) external returns (uint256 assets);
```

### View Functions

```solidity
function totalAssets() external view returns (uint256);
function convertToShares(uint256 assets) external view returns (uint256);
function convertToAssets(uint256 shares) external view returns (uint256);
```

## Wrfcoin Implementation

### Weather Data Staking Vault

Wrfcoin uses ERC-4626 vaults for weather data staking and yield generation:

```javascript
// Deposit WRF tokens into weather data vault
const wrf_amount = ethers.utils.parseEther('100');
const shares = await weatherVault.deposit(wrf_amount, userAddress);
```

### Yield Strategies

1. **Data Mining Rewards**: Earn from weather data submissions
2. **Oracle Fees**: Share in oracle service revenue
3. **Validation Rewards**: Participate in data validation consensus
4. **Cross-chain Yields**: Generate returns from multi-chain operations

## Use Cases

### 1. Liquid Staking

Users can stake WRF tokens while maintaining liquidity through vault shares.

### 2. Yield Aggregation

Automatically optimize returns across multiple yield sources.

### 3. Risk Management

Diversify exposure across different weather data markets.

### 4. Governance Participation

Vault shares can represent voting power in governance decisions.

## Integration Examples

### Basic Vault Interaction

```javascript
const vault = new ethers.Contract(vaultAddress, ERC4626_ABI, signer);

// Check vault metrics
const totalAssets = await vault.totalAssets();
const sharePrice = await vault.convertToAssets(ethers.utils.parseEther('1'));

// Deposit assets
const depositTx = await vault.deposit(amount, recipient);
```

### Advanced Strategies

```javascript
// Auto-compound rewards
async function autoCompound() {
  const rewards = await vault.claimableRewards(userAddress);
  if (rewards.gt(0)) {
    await vault.reinvestRewards();
  }
}
```

## Security Considerations

- **Reentrancy Protection**: Implement proper guards
- **Share Inflation Attacks**: Use virtual shares/assets
- **Slippage Protection**: Set appropriate limits
- **Emergency Withdrawals**: Implement circuit breakers

## Benefits for Weather Networks

1. **Capital Efficiency**: Maximize utility of staked tokens
2. **Network Security**: Align incentives through staking
3. **Scalability**: Support larger validator sets
4. **Interoperability**: Easy integration with DeFi ecosystem

## Related Standards

- **ERC-20**: Base token standard for vault shares
- **EIP-2612**: Permit functionality for gasless approvals
- **ERC-165**: Interface detection support
