# ERC-1155: Multi-Token Standard

## Overview

ERC-1155 is a versatile token standard that supports multiple token types within a single contract,
including both fungible and non-fungible tokens. This standard is ideal for weather data systems
that need to manage various types of assets efficiently.

## Key Advantages

- **Gas Efficiency**: Batch operations reduce transaction costs
- **Flexibility**: Mix fungible and non-fungible tokens in one contract
- **Atomic Swaps**: Transfer multiple token types in a single transaction
- **Rich Metadata**: Comprehensive token information support

## Core Interface

### Essential Functions

```solidity
function balanceOf(address account, uint256 id) external view returns (uint256);
function balanceOfBatch(address[] accounts, uint256[] ids) external view returns (uint256[]);
function safeTransferFrom(address from, address to, uint256 id, uint256 amount, bytes data) external;
function safeBatchTransferFrom(address from, address to, uint256[] ids, uint256[] amounts, bytes data) external;
```

### Approval Functions

```solidity
function setApprovalForAll(address operator, bool approved) external;
function isApprovedForAll(address account, address operator) external view returns (bool);
```

## Wrfcoin Multi-Token Applications

### 1. Weather Data Assets

Different token types for various weather data categories:

```javascript
// Token ID scheme for weather data
const TOKEN_TYPES = {
  TEMPERATURE_DATA: 1, // Fungible - units of temperature readings
  RAINFALL_DATA: 2, // Fungible - rainfall measurement units
  EXTREME_EVENT_NFT: 1001, // Non-fungible - unique weather events
  SENSOR_CERTIFICATE: 1002, // Non-fungible - sensor validation certificates
  COMPUTATION_CREDITS: 3, // Fungible - computational resource credits
  ORACLE_SHARES: 4, // Fungible - oracle service participation shares
};

// Mint different token types
await wrfMultiToken.mintBatch(
  userAddress,
  [TOKEN_TYPES.TEMPERATURE_DATA, TOKEN_TYPES.EXTREME_EVENT_NFT],
  [100, 1], // 100 temperature units + 1 unique event NFT
  '0x'
);
```

### 2. Reward Distribution System

Efficient distribution of multiple reward types:

```javascript
// Batch distribute rewards to validators
const validators = ['0x123...', '0x456...', '0x789...'];
const rewardTypes = [TOKEN_TYPES.COMPUTATION_CREDITS, TOKEN_TYPES.ORACLE_SHARES];
const amounts = [
  [50, 75],
  [25, 30],
  [40, 45],
]; // Credits and shares per validator

await wrfMultiToken.batchMintToMultiple(validators, rewardTypes, amounts);
```

### 3. Data Marketplace

Unified marketplace for all asset types:

```javascript
// List multiple assets for sale
const marketplace = new ethers.Contract(marketplaceAddress, MARKETPLACE_ABI, signer);

await wrfMultiToken.setApprovalForAll(marketplaceAddress, true);
await marketplace.batchListAssets(
  contractAddress,
  [TOKEN_TYPES.TEMPERATURE_DATA, TOKEN_TYPES.SENSOR_CERTIFICATE],
  [50, 1],
  [ethers.utils.parseEther('0.1'), ethers.utils.parseEther('5.0')]
);
```

## Token ID Schema

### Wrfcoin Token Categories

```javascript
const TOKEN_CATEGORIES = {
  // Fungible Tokens (1-999)
  FUNGIBLE_START: 1,
  WEATHER_DATA_UNITS: 1,
  COMPUTATION_CREDITS: 2,
  ORACLE_PARTICIPATION: 3,
  VALIDATION_TOKENS: 4,

  // Semi-Fungible Tokens (1000-1999)
  SEMI_FUNGIBLE_START: 1000,
  LOCATION_SPECIFIC_DATA: 1000,
  TIME_BOUNDED_LICENSES: 1001,

  // Non-Fungible Tokens (2000+)
  NFT_START: 2000,
  EXTREME_WEATHER_EVENTS: 2000,
  SENSOR_CERTIFICATES: 2001,
  ACHIEVEMENT_BADGES: 2002,
  COMPUTATIONAL_RESULTS: 2003,
};
```

## Metadata Management

### URI Pattern

```javascript
// Dynamic URI generation based on token type
function uri(uint256 tokenId) public view returns (string memory) {
    if (tokenId < 1000) {
        return string(abi.encodePacked(baseURI, "fungible/", tokenId.toString()));
    } else if (tokenId < 2000) {
        return string(abi.encodePacked(baseURI, "semi-fungible/", tokenId.toString()));
    } else {
        return string(abi.encodePacked(baseURI, "nft/", tokenId.toString()));
    }
}
```

### Metadata Examples

```json
{
  "name": "Temperature Data Units",
  "description": "Fungible tokens representing verified temperature measurements",
  "image": "ipfs://QmTemperature...",
  "properties": {
    "category": "weather_data",
    "measurement_type": "temperature",
    "precision": "0.1°C",
    "verification_level": "high"
  }
}
```

## Implementation Patterns

### Batch Minting with Access Control

```solidity
function batchMintRewards(
    address[] calldata recipients,
    uint256[] calldata tokenIds,
    uint256[] calldata amounts
) external onlyRewardDistributor {
    require(recipients.length == tokenIds.length, "Length mismatch");
    require(tokenIds.length == amounts.length, "Length mismatch");

    for (uint256 i = 0; i < recipients.length; i++) {
        _mint(recipients[i], tokenIds[i], amounts[i], "");
    }
}
```

### Supply Management

```solidity
mapping(uint256 => uint256) public tokenSupply;
mapping(uint256 => uint256) public maxSupply;

function mint(address to, uint256 id, uint256 amount) external {
    require(tokenSupply[id] + amount <= maxSupply[id], "Exceeds max supply");
    tokenSupply[id] += amount;
    _mint(to, id, amount, "");
}
```

## Use Cases in Weather Networks

### 1. Unified Asset Management

Manage all network assets in a single contract for efficiency.

### 2. Complex Reward Systems

Distribute multiple types of rewards based on contribution quality.

### 3. Data Licensing

Create fungible licenses for data access with varying privileges.

### 4. Achievement Systems

Award unique badges while maintaining fungible credit systems.

## Gas Optimization Strategies

### Packed Transfers

```javascript
// Transfer multiple assets in one transaction
await wrfMultiToken.safeBatchTransferFrom(
  fromAddress,
  toAddress,
  [1, 2, 1001], // Temperature data, rainfall data, extreme event NFT
  [25, 15, 1], // Amounts
  '0x'
);
```

### Batch Operations

```javascript
// Check multiple balances efficiently
const balances = await wrfMultiToken.balanceOfBatch(
  [userAddress, userAddress, userAddress],
  [TOKEN_TYPES.TEMPERATURE_DATA, TOKEN_TYPES.ORACLE_SHARES, TOKEN_TYPES.EXTREME_EVENT_NFT]
);
```

## Security Considerations

- **Reentrancy Protection**: Implement proper guards for all transfer functions
- **Supply Tracking**: Monitor token supply for fungible tokens
- **Access Control**: Role-based minting and burning permissions
- **Approval Management**: Clear approval patterns for batch operations

## Integration Examples

### DeFi Integration

```javascript
// Provide liquidity with multiple token types
await liquidityPool.addLiquidityMulti(
  wrfMultiTokenAddress,
  [TOKEN_TYPES.WEATHER_DATA, TOKEN_TYPES.COMPUTATION_CREDITS],
  [1000, 500]
);
```

### Cross-chain Bridging

```javascript
// Bridge multiple assets to another chain
await crossChainBridge.bridgeMultipleAssets(
  destinationChainId,
  wrfMultiTokenAddress,
  tokenIds,
  amounts,
  recipientAddress
);
```

## Related Standards

- **ERC-165**: Interface detection support
- **ERC-721**: Single NFT standard for comparison
- **ERC-20**: Fungible token standard for comparison
- **EIP-2981**: Royalty standard for NFT components
