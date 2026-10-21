# Carbon Credit Market API Documentation

## Overview

The Carbon Credit Market API provides comprehensive access to Wrfcoin's carbon credit
infrastructure, including token operations, AMM trading, yield farming, insurance, and oracle data
feeds. This RESTful API is designed for developers building applications on top of the carbon credit
market.

## Base URL

- **Testnet**: `https://testnet-api.wrfcoin.org/api`
- **Mainnet**: `https://api.wrfcoin.org/api`

## Authentication

The API uses private key signatures for transaction operations. For read operations, no
authentication is required.

## Rate Limits

- **General operations**: 100 requests per 15 minutes per IP
- **Trading operations**: 10 requests per minute per IP
- **Oracle data**: 50 requests per 15 minutes per IP

## Request/Response Format

All API responses follow this format:

```json
{
  "success": boolean,
  "data": object | array | null,
  "error": string | null,
  "timestamp": "ISO 8601 timestamp",
  "requestId": "unique-request-id"
}
```

## Error Handling

HTTP status codes indicate the result:

- `200`: Success
- `400`: Bad Request (validation errors)
- `401`: Unauthorized
- `403`: Forbidden
- `404`: Not Found
- `429`: Too Many Requests (rate limited)
- `500`: Internal Server Error

Error responses include details:

```json
{
  "success": false,
  "error": "Detailed error message",
  "timestamp": "2024-01-01T00:00:00.000Z",
  "requestId": "req-123"
}
```

---

# Carbon Credit Token Operations

## Get Carbon Credit Batches

Retrieve carbon credit batches with pagination and filtering.

**Endpoint**: `GET /carbon-credits/batches`

**Query Parameters**:

- `page` (integer, optional): Page number (default: 1)
- `limit` (integer, optional): Items per page (default: 20, max: 100)
- `projectType` (string, optional): Filter by project type
- `registryStandard` (string, optional): Filter by registry standard
- `vintage` (integer, optional): Filter by vintage year
- `isActive` (boolean, optional): Filter by active status

**Example Request**:

```bash
curl "https://api.wrfcoin.org/api/carbon-credits/batches?page=1&limit=10&projectType=forestry&vintage=2023"
```

**Example Response**:

```json
{
  "success": true,
  "data": {
    "batches": [
      {
        "tokenId": 1,
        "totalSupply": "1000000",
        "immobilizedSupply": "800000",
        "retiredSupply": "200000",
        "projectId": "VCS-001-2023",
        "vintage": 2023,
        "methodology": "VM0006",
        "projectType": "forestry",
        "registryStandard": "VCS",
        "location": "Brazil, Amazon",
        "issuer": "0x1234...",
        "issuanceDate": 1672531200,
        "expiryDate": 1735689600,
        "certificationHash": "QmX...",
        "registryUrl": "https://registry.verra.org/...",
        "isActive": true
      }
    ],
    "pagination": {
      "currentPage": 1,
      "totalPages": 5,
      "totalItems": 50,
      "itemsPerPage": 10
    }
  },
  "timestamp": "2024-01-01T00:00:00.000Z",
  "requestId": "req-123"
}
```

## Get Carbon Credit Batch Details

Retrieve detailed information about a specific carbon credit batch.

**Endpoint**: `GET /carbon-credits/batches/{tokenId}`

**Path Parameters**:

- `tokenId` (integer, required): Token ID of the batch

**Example Request**:

```bash
curl "https://api.wrfcoin.org/api/carbon-credits/batches/1"
```

**Example Response**:

```json
{
  "success": true,
  "data": {
    "tokenId": 1,
    "totalSupply": "1000000",
    "uri": "https://metadata.wrfcoin.org/carbon-credits/1",
    "projectId": "VCS-001-2023",
    "vintage": 2023,
    "methodology": "VM0006",
    "projectType": "forestry",
    "registryStandard": "VCS",
    "location": "Brazil, Amazon",
    "issuer": "0x1234567890123456789012345678901234567890",
    "issuanceDate": 1672531200,
    "expiryDate": 1735689600,
    "certificationHash": "QmXoypizjW3WknFiJnKLwHCnL72vedxjQkDDP1mXWo6uco",
    "registryUrl": "https://registry.verra.org/myModule/rpt/myRpt.asp?r=206&h=0",
    "isActive": true,
    "immobilizedSupply": "800000",
    "retiredSupply": "200000"
  },
  "timestamp": "2024-01-01T00:00:00.000Z",
  "requestId": "req-124"
}
```

## Retire Carbon Credits

Permanently retire (burn) carbon credits with an on-chain retirement certificate.

**Endpoint**: `POST /carbon-credits/batches/{tokenId}/retire`

**Path Parameters**:

- `tokenId` (integer, required): Token ID of the batch to retire from

**Request Body**:

```json
{
  "amount": "100.5",
  "retirementReason": "Corporate carbon neutrality offset for Q4 2023 operations",
  "beneficiary": "Acme Corporation",
  "privateKey": "0x1234567890123456789012345678901234567890123456789012345678901234"
}
```

**Request Body Fields**:

- `amount` (string, required): Amount of credits to retire (in kg CO2e)
- `retirementReason` (string, required): Reason for retirement (10-500 characters)
- `beneficiary` (string, optional): Beneficiary of the retirement
- `privateKey` (string, required): Private key for transaction signing

**Example Response**:

```json
{
  "success": true,
  "data": {
    "transactionHash": "0xabc123...",
    "blockNumber": 12345678,
    "tokenId": 1,
    "amountRetired": "100.5",
    "retirementReason": "Corporate carbon neutrality offset for Q4 2023 operations",
    "beneficiary": "Acme Corporation",
    "gasUsed": "125000"
  },
  "timestamp": "2024-01-01T00:00:00.000Z",
  "requestId": "req-125"
}
```

## Transfer Carbon Credits

Transfer carbon credits between addresses.

**Endpoint**: `POST /carbon-credits/transfer`

**Request Body**:

```json
{
  "tokenId": 1,
  "to": "0x9876543210987654321098765432109876543210",
  "amount": "50.0",
  "privateKey": "0x1234567890123456789012345678901234567890123456789012345678901234"
}
```

**Request Body Fields**:

- `tokenId` (integer, required): Token ID to transfer
- `to` (string, required): Ethereum address of recipient
- `amount` (string, required): Amount to transfer
- `privateKey` (string, required): Private key for transaction signing

**Example Response**:

```json
{
  "success": true,
  "data": {
    "transactionHash": "0xdef456...",
    "blockNumber": 12345679,
    "from": "0x1234567890123456789012345678901234567890",
    "to": "0x9876543210987654321098765432109876543210",
    "tokenId": 1,
    "amount": "50.0",
    "gasUsed": "85000"
  },
  "timestamp": "2024-01-01T00:00:00.000Z",
  "requestId": "req-126"
}
```

---

# AMM Trading Operations

## Get AMM Pools

Retrieve all available AMM pools with their current state.

**Endpoint**: `GET /amm/pools`

**Example Request**:

```bash
curl "https://api.wrfcoin.org/api/amm/pools"
```

**Example Response**:

```json
{
  "success": true,
  "data": {
    "pools": [
      {
        "address": "0xpool1234...",
        "token0": "0xWRF...",
        "token1": "0xCARBON...",
        "reserve0": "1000000000000000000000",
        "reserve1": "500000000000000000000",
        "fee": "30",
        "tvl": "1500.0"
      }
    ]
  },
  "timestamp": "2024-01-01T00:00:00.000Z",
  "requestId": "req-127"
}
```

## Get Swap Quote

Get a quote for token swap without executing the transaction.

**Endpoint**: `GET /amm/quote`

**Query Parameters**:

- `tokenIn` (string, required): Address of input token
- `tokenOut` (string, required): Address of output token
- `amountIn` (string, required): Amount of input token

**Example Request**:

```bash
curl "https://api.wrfcoin.org/api/amm/quote?tokenIn=0xWRF...&tokenOut=0xCARBON...&amountIn=100"
```

**Example Response**:

```json
{
  "success": true,
  "data": {
    "amountOut": "98.5",
    "priceImpact": "0.15",
    "fee": "0.3",
    "route": ["0xWRF...", "0xCARBON..."]
  },
  "timestamp": "2024-01-01T00:00:00.000Z",
  "requestId": "req-128"
}
```

## Execute Swap

Execute a token swap through the AMM.

**Endpoint**: `POST /amm/swap`

**Request Body**:

```json
{
  "tokenIn": "0xWRF1234567890123456789012345678901234567890",
  "tokenOut": "0xCARBON1234567890123456789012345678901234567890",
  "amountIn": "100.0",
  "amountOutMin": "95.0",
  "deadline": 1672617600,
  "privateKey": "0x1234567890123456789012345678901234567890123456789012345678901234"
}
```

**Request Body Fields**:

- `tokenIn` (string, required): Address of input token
- `tokenOut` (string, required): Address of output token
- `amountIn` (string, required): Amount of input token
- `amountOutMin` (string, required): Minimum amount of output token
- `deadline` (integer, required): Unix timestamp deadline
- `privateKey` (string, required): Private key for transaction signing

**Example Response**:

```json
{
  "success": true,
  "data": {
    "transactionHash": "0xswap123...",
    "blockNumber": 12345680,
    "tokenIn": "0xWRF...",
    "tokenOut": "0xCARBON...",
    "amountIn": "100.0",
    "amountOut": "98.2",
    "gasUsed": "150000"
  },
  "timestamp": "2024-01-01T00:00:00.000Z",
  "requestId": "req-129"
}
```

## Add Liquidity

Add liquidity to an AMM pool.

**Endpoint**: `POST /amm/add-liquidity`

**Request Body**:

```json
{
  "tokenA": "0xWRF1234567890123456789012345678901234567890",
  "tokenB": "0xCARBON1234567890123456789012345678901234567890",
  "amountADesired": "1000.0",
  "amountBDesired": "500.0",
  "amountAMin": "950.0",
  "amountBMin": "475.0",
  "deadline": 1672617600,
  "privateKey": "0x1234567890123456789012345678901234567890123456789012345678901234"
}
```

**Example Response**:

```json
{
  "success": true,
  "data": {
    "transactionHash": "0xliquidity123...",
    "blockNumber": 12345681,
    "tokenA": "0xWRF...",
    "tokenB": "0xCARBON...",
    "amountA": "1000.0",
    "amountB": "500.0",
    "liquidity": "707.106781186547",
    "gasUsed": "200000"
  },
  "timestamp": "2024-01-01T00:00:00.000Z",
  "requestId": "req-130"
}
```

---

# Yield Farming Operations

## Get Yield Farming Pools

Retrieve all available yield farming pools with their reward rates and APY.

**Endpoint**: `GET /yield-farming/pools`

**Example Response**:

```json
{
  "success": true,
  "data": {
    "pools": [
      {
        "address": "0xfarm1234...",
        "liquidityPool": "0xpool1234...",
        "carbonBasketToken": "0xbasket1234...",
        "baseRewardRate": "1000000000000000000",
        "qualityMultiplier": "1500000000000000000",
        "vintageBonus": "500",
        "minStakeDuration": "2592000",
        "farmType": "premium",
        "isActive": true,
        "rewardTokens": ["0xWRF...", "0xQUALITY..."]
      }
    ]
  },
  "timestamp": "2024-01-01T00:00:00.000Z",
  "requestId": "req-131"
}
```

## Get Staking Position

Get user's staking position in a specific farm.

**Endpoint**: `GET /yield-farming/position/{farmAddress}/{userAddress}`

**Example Response**:

```json
{
  "success": true,
  "data": {
    "stakedAmount": "1000.0",
    "pendingRewards": "25.5",
    "lockEndTime": 1675209600,
    "qualityMultiplier": "1.5",
    "totalRewardsClaimed": "150.0"
  },
  "timestamp": "2024-01-01T00:00:00.000Z",
  "requestId": "req-132"
}
```

## Stake Tokens

Stake LP tokens in a yield farming pool.

**Endpoint**: `POST /yield-farming/stake`

**Request Body**:

```json
{
  "farmAddress": "0xfarm1234567890123456789012345678901234567890",
  "amount": "500.0",
  "lockDuration": 90,
  "privateKey": "0x1234567890123456789012345678901234567890123456789012345678901234"
}
```

**Request Body Fields**:

- `farmAddress` (string, required): Address of the farming pool
- `amount` (string, required): Amount of LP tokens to stake
- `lockDuration` (integer, optional): Lock duration in days for bonus rewards
- `privateKey` (string, required): Private key for transaction signing

**Example Response**:

```json
{
  "success": true,
  "data": {
    "transactionHash": "0xstake123...",
    "blockNumber": 12345682,
    "farmAddress": "0xfarm1234...",
    "amount": "500.0",
    "lockDuration": 90,
    "gasUsed": "180000"
  },
  "timestamp": "2024-01-01T00:00:00.000Z",
  "requestId": "req-133"
}
```

## Claim Rewards

Claim accumulated farming rewards from a pool.

**Endpoint**: `POST /yield-farming/claim-rewards`

**Request Body**:

```json
{
  "farmAddress": "0xfarm1234567890123456789012345678901234567890",
  "privateKey": "0x1234567890123456789012345678901234567890123456789012345678901234"
}
```

**Example Response**:

```json
{
  "success": true,
  "data": {
    "transactionHash": "0xclaim123...",
    "blockNumber": 12345683,
    "farmAddress": "0xfarm1234...",
    "user": "0x1234...",
    "rewards": [
      {
        "token": "0xWRF...",
        "amount": "25.5"
      },
      {
        "token": "0xQUALITY...",
        "amount": "5.2"
      }
    ],
    "gasUsed": "120000"
  },
  "timestamp": "2024-01-01T00:00:00.000Z",
  "requestId": "req-134"
}
```

---

# Insurance Operations

## Get Insurance Policies

Retrieve available insurance policies and coverage options.

**Endpoint**: `GET /insurance/policies`

**Example Response**:

```json
{
  "success": true,
  "data": {
    "policies": [
      {
        "coverageType": "PROJECT_FAILURE",
        "premiumRate": "0.05",
        "maxCoverage": "1000000",
        "minDuration": 30,
        "maxDuration": 1825,
        "description": "Coverage against project failure or non-delivery"
      },
      {
        "coverageType": "VERIFICATION_RISK",
        "premiumRate": "0.03",
        "maxCoverage": "500000",
        "minDuration": 90,
        "maxDuration": 365,
        "description": "Coverage against verification failures or fraud"
      }
    ]
  },
  "timestamp": "2024-01-01T00:00:00.000Z",
  "requestId": "req-135"
}
```

## Calculate Insurance Premium

Calculate premium for insurance coverage.

**Endpoint**: `GET /insurance/premium-quote`

**Query Parameters**:

- `coverageType` (string, required): Type of coverage
- `tokenId` (integer, required): Carbon credit token ID
- `coverageAmount` (string, required): Coverage amount
- `duration` (integer, required): Coverage duration in days

**Example Request**:

```bash
curl "https://api.wrfcoin.org/api/insurance/premium-quote?coverageType=PROJECT_FAILURE&tokenId=1&coverageAmount=1000&duration=365"
```

**Example Response**:

```json
{
  "success": true,
  "data": {
    "premium": "50.0",
    "premiumBreakdown": {
      "basePremium": "50.0",
      "riskMultiplier": "1.2",
      "qualityDiscount": "0.9",
      "finalPremium": "54.0"
    }
  },
  "timestamp": "2024-01-01T00:00:00.000Z",
  "requestId": "req-136"
}
```

## Create Insurance Policy

Create a new insurance policy.

**Endpoint**: `POST /insurance/create-policy`

**Request Body**:

```json
{
  "coverageType": "PROJECT_FAILURE",
  "tokenId": 1,
  "coverageAmount": "1000.0",
  "duration": 365,
  "privateKey": "0x1234567890123456789012345678901234567890123456789012345678901234"
}
```

**Example Response**:

```json
{
  "success": true,
  "data": {
    "transactionHash": "0xinsurance123...",
    "blockNumber": 12345684,
    "policyId": "POL-001-2024",
    "coverageType": "PROJECT_FAILURE",
    "tokenId": 1,
    "coverageAmount": "1000.0",
    "duration": 365,
    "premium": "54.0",
    "gasUsed": "250000"
  },
  "timestamp": "2024-01-01T00:00:00.000Z",
  "requestId": "req-137"
}
```

---

# Oracle Data Feeds

## Get Carbon Credit Prices

Retrieve current carbon credit prices from oracle feeds.

**Endpoint**: `GET /oracles/carbon-prices`

**Query Parameters**:

- `projectType` (string, optional): Filter by project type
- `vintage` (integer, optional): Filter by vintage year
- `registryStandard` (string, optional): Filter by registry standard

**Example Request**:

```bash
curl "https://api.wrfcoin.org/api/oracles/carbon-prices?projectType=forestry&vintage=2023"
```

**Example Response**:

```json
{
  "success": true,
  "data": [
    {
      "projectType": "forestry",
      "vintage": 2023,
      "registryStandard": "VCS",
      "priceUSD": "12.50",
      "priceWRF": "25.0",
      "confidence": 95,
      "lastUpdated": "2024-01-01T00:00:00.000Z"
    }
  ],
  "timestamp": "2024-01-01T00:00:00.000Z",
  "requestId": "req-138"
}
```

## Get Weather Data

Retrieve weather data for risk assessment and derivatives.

**Endpoint**: `GET /oracles/weather-data`

**Query Parameters**:

- `location` (string, required): Geographic location
- `startDate` (string, required): Start date (ISO 8601)
- `endDate` (string, required): End date (ISO 8601)
- `metrics` (string, optional): Comma-separated list of metrics

**Example Request**:

```bash
curl "https://api.wrfcoin.org/api/oracles/weather-data?location=Brazil,Amazon&startDate=2024-01-01&endDate=2024-01-31&metrics=temperature,humidity,rainfall"
```

**Example Response**:

```json
{
  "success": true,
  "data": [
    {
      "location": "Brazil, Amazon",
      "timestamp": "2024-01-01T12:00:00.000Z",
      "temperature": 28.5,
      "humidity": 85.2,
      "pressure": 1013.25,
      "windSpeed": 3.2,
      "rainfall": 0.0,
      "co2Level": 415.3,
      "dataSource": "NOAA",
      "confidence": 98
    }
  ],
  "timestamp": "2024-01-01T00:00:00.000Z",
  "requestId": "req-139"
}
```

## Get Verification Status

Get project verification status from registries.

**Endpoint**: `GET /oracles/verification/{registryStandard}/{projectId}`

**Path Parameters**:

- `registryStandard` (string, required): Registry standard (VCS, GS, CDM, etc.)
- `projectId` (string, required): Project identifier

**Example Request**:

```bash
curl "https://api.wrfcoin.org/api/oracles/verification/VCS/VCS-001-2023"
```

**Example Response**:

```json
{
  "success": true,
  "data": {
    "projectId": "VCS-001-2023",
    "registryStandard": "VCS",
    "status": "VERIFIED",
    "lastUpdate": "2024-01-01T00:00:00.000Z",
    "verificationHash": "0xverify123...",
    "verifierName": "Verra Registry",
    "validUntil": "2025-01-01T00:00:00.000Z"
  },
  "timestamp": "2024-01-01T00:00:00.000Z",
  "requestId": "req-140"
}
```

---

# SDK Usage Examples

## Installation

```bash
npm install @wrfcoin/carbon-credit-sdk
```

## Basic Usage

```typescript
import { CarbonCreditSDK } from '@wrfcoin/carbon-credit-sdk';

// Initialize SDK
const sdk = new CarbonCreditSDK({
  apiBaseUrl: 'https://api.wrfcoin.org/api',
});

// Get carbon credit batches
const batches = await sdk.getCarbonCreditBatches(
  {
    page: 1,
    limit: 10,
  },
  {
    projectType: 'forestry',
    vintage: 2023,
  }
);

// Retire carbon credits
const retirement = await sdk.retireCarbonCredits(1, {
  amount: '100.0',
  retirementReason: 'Corporate offset for Q4 2023',
  beneficiary: 'Acme Corp',
  privateKey: process.env.PRIVATE_KEY!,
});

// Execute AMM swap
const swap = await sdk.swapTokens({
  tokenIn: '0xWRF...',
  tokenOut: '0xCARBON...',
  amountIn: '100.0',
  amountOutMin: '95.0',
  deadline: CarbonCreditSDK.generateDeadline(20),
  privateKey: process.env.PRIVATE_KEY!,
});

// Stake in yield farming
const stake = await sdk.stakeTokens({
  farmAddress: '0xfarm...',
  amount: '500.0',
  lockDuration: 90,
  privateKey: process.env.PRIVATE_KEY!,
});
```

## Error Handling

```typescript
try {
  const result = await sdk.getCarbonCreditBatch(123);
} catch (error) {
  if (error.statusCode === 404) {
    console.log('Carbon credit batch not found');
  } else if (error.statusCode === 429) {
    console.log('Rate limit exceeded, retry later');
  } else {
    console.log('API error:', error.message);
  }
}
```

## Utilities

```typescript
import { CarbonCreditSDK } from '@wrfcoin/carbon-credit-sdk';

// Validate Ethereum address
const isValid = CarbonCreditSDK.isValidAddress('0x1234...');

// Generate deadline (20 minutes from now)
const deadline = CarbonCreditSDK.generateDeadline(20);

// Convert between Wei and Ether
const weiAmount = CarbonCreditSDK.parseEther('100.0');
const etherAmount = CarbonCreditSDK.formatEther(weiAmount);

// Get address from private key
const address = CarbonCreditSDK.getAddressFromPrivateKey(privateKey);
```

---

# Rate Limits and Best Practices

## Rate Limits

- Respect rate limits to ensure service availability
- Implement exponential backoff for retries
- Use pagination for large datasets
- Cache data when appropriate

## Security Best Practices

- Never expose private keys in client-side code
- Use environment variables for sensitive data
- Validate all inputs before sending to API
- Implement proper error handling
- Use HTTPS for all API calls

## Performance Optimization

- Use appropriate pagination parameters
- Implement caching for frequently accessed data
- Batch operations when possible
- Use WebSockets for real-time updates (when available)

---

# Support and Resources

- **Documentation**: https://docs.wrfcoin.org
- **SDK Repository**: https://github.com/wrfcoin/carbon-credit-sdk
- **Support**: support@wrfcoin.org
- **Discord**: https://discord.gg/wrfcoin

For additional examples and advanced usage, see the SDK repository and documentation website.
