# ERC-721: Non-Fungible Tokens

## Overview

ERC-721 is the standard for non-fungible tokens (NFTs) on Ethereum and compatible blockchains. Each
token is unique and represents ownership of a distinct digital asset, making it perfect for weather
data certificates, computational results, and unique environmental records.

## Key Characteristics

- **Uniqueness**: Each token has a distinct tokenId
- **Ownership**: Clear provenance and transfer history
- **Metadata**: Rich information associated with each token
- **Interoperability**: Compatible across platforms and marketplaces

## Core Interface

### Essential Functions

```solidity
function balanceOf(address owner) external view returns (uint256 balance);
function ownerOf(uint256 tokenId) external view returns (address owner);
function transferFrom(address from, address to, uint256 tokenId) external;
function approve(address to, uint256 tokenId) external;
function getApproved(uint256 tokenId) external view returns (address operator);
```

### Metadata Extension

```solidity
function tokenURI(uint256 tokenId) external view returns (string memory);
```

## Wrfcoin NFT Applications

### 1. Weather Data Certificates

Each significant weather event or data contribution can be minted as an NFT:

```javascript
// Mint weather event NFT
const weatherNFT = await wrfNFTContract.mintWeatherEvent(recipientAddress, {
  temperature: '45.2°C',
  location: 'Phoenix, AZ',
  timestamp: '2024-07-15T14:30:00Z',
  severity: 'extreme_heat',
  verificationScore: 98.5,
});
```

### 2. Computational NFTs

Unique computational results and AI model outputs:

```javascript
// Mint computational result
const computeNFT = await wrfNFTContract.mintComputationalResult(
  modelHash,
  inputDataHash,
  resultHash,
  computationMetadata
);
```

### 3. Validator Achievements

Recognition for network participants and validators:

```javascript
// Mint validator achievement
const achievementNFT = await wrfNFTContract.mintAchievement(
  validatorAddress,
  'data_accuracy_champion',
  achievementData
);
```

## Metadata Standards

### Weather Data NFT Metadata

```json
{
  "name": "Extreme Heat Event - Phoenix 2024",
  "description": "Record-breaking temperature data verified by Wrfcoin network",
  "image": "ipfs://QmX...",
  "attributes": [
    {
      "trait_type": "Temperature",
      "value": "45.2°C"
    },
    {
      "trait_type": "Location",
      "value": "Phoenix, AZ"
    },
    {
      "trait_type": "Verification Score",
      "value": 98.5
    },
    {
      "trait_type": "Rarity",
      "value": "Legendary"
    }
  ],
  "external_url": "https://wrfcoin.network/weather-events/...",
  "animation_url": "ipfs://QmY..."
}
```

## Implementation Best Practices

### Gas Optimization

```solidity
// Use packed structs for metadata
struct WeatherData {
    uint32 temperature;  // Scaled by 100
    uint32 humidity;     // Percentage * 100
    uint64 timestamp;
    uint16 locationId;
}
```

### Batch Operations

```solidity
function batchMint(address[] calldata recipients, uint256[] calldata amounts)
    external onlyMinter {
    require(recipients.length == amounts.length, "Length mismatch");

    for (uint256 i = 0; i < recipients.length; i++) {
        _mint(recipients[i], amounts[i]);
    }
}
```

## Use Cases in Weather Networks

### 1. Data Provenance

Track the origin and validation history of weather measurements.

### 2. Achievement Systems

Reward network participants with unique recognition tokens.

### 3. Collectible Markets

Create trading markets for rare weather events and data.

### 4. Governance Rights

Use NFTs to represent voting power or special privileges.

## Security Considerations

- **Reentrancy Protection**: Implement proper guards for transfers
- **Integer Overflow**: Use SafeMath or Solidity 0.8+
- **Access Control**: Implement role-based permissions
- **Metadata Immutability**: Consider IPFS for permanent storage

## Integration Examples

### Basic NFT Operations

```javascript
const nftContract = new ethers.Contract(contractAddress, ERC721_ABI, signer);

// Check ownership
const owner = await nftContract.ownerOf(tokenId);
const balance = await nftContract.balanceOf(userAddress);

// Transfer NFT
await nftContract.transferFrom(fromAddress, toAddress, tokenId);

// Get metadata
const tokenURI = await nftContract.tokenURI(tokenId);
const metadata = await fetch(tokenURI).then(r => r.json());
```

### Marketplace Integration

```javascript
// Approve marketplace for trading
await nftContract.approve(marketplaceAddress, tokenId);

// List for sale
await marketplace.listNFT(contractAddress, tokenId, price);
```

## Related Standards

- **ERC-165**: Interface detection
- **ERC-721Enumerable**: Token enumeration extension
- **ERC-2981**: NFT royalty standard
- **EIP-2309**: Consecutive transfer extension
