# WrfView API Integration Guide

This document outlines how WrfView integrates with Wrfcoin Core's API endpoints to provide enhanced
functionality. Developers building extensions for WrfView or integrating with the Wrfcoin ecosystem
should follow these integration patterns.

## Core API Integration

### Authentication & Authorization

WrfView uses a secure authentication flow to interact with protected Wrfcoin Core endpoints:

```javascript
// Example authentication
const authToken = await WrfViewAuth.getToken({
  walletAddress: currentWallet.address,
  signature: await wallet.signMessage('WrfView Auth Request'),
});

// Use in subsequent requests
const headers = {
  Authorization: `Bearer ${authToken}`,
  'Content-Type': 'application/json',
};
```

### REST API Examples

#### Wallet Operations

```javascript
// Get wallet balances
async function getWalletBalances(address) {
  const response = await fetch(`${nodeUrl}/api/wallet/${address}`, { headers });
  if (!response.ok) throw new Error('Failed to fetch wallet data');
  return await response.json();
}

// Send transaction
async function sendTransaction(txData) {
  const response = await fetch(`${nodeUrl}/api/transactions`, {
    method: 'POST',
    headers,
    body: JSON.stringify(txData),
  });
  if (!response.ok) throw new Error('Failed to send transaction');
  return await response.json();
}
```

#### Blockchain Data

```javascript
// Get latest blocks
async function getLatestBlocks(limit = 10) {
  const response = await fetch(`${nodeUrl}/api/blocks?limit=${limit}`, { headers });
  if (!response.ok) throw new Error('Failed to fetch blocks');
  return await response.json();
}

// Get block by hash
async function getBlockByHash(hash) {
  const response = await fetch(`${nodeUrl}/api/blocks/hash/${hash}`, { headers });
  if (!response.ok) throw new Error('Failed to fetch block');
  return await response.json();
}
```

#### Governance Interactions

```javascript
// Submit governance proposal
async function submitProposal(proposalData) {
  const response = await fetch(`${nodeUrl}/api/governance/proposals`, {
    method: 'POST',
    headers,
    body: JSON.stringify(proposalData),
  });
  if (!response.ok) throw new Error('Failed to submit proposal');
  return await response.json();
}

// Cast vote
async function castVote(proposalId, voteData) {
  const response = await fetch(`${nodeUrl}/api/governance/proposals/${proposalId}/votes`, {
    method: 'POST',
    headers,
    body: JSON.stringify(voteData),
  });
  if (!response.ok) throw new Error('Failed to cast vote');
  return await response.json();
}
```

### WebSocket Integration

WrfView uses WebSockets for real-time updates from the Wrfcoin blockchain:

```javascript
// Initialize WebSocket connection
function initializeWebSocket() {
  const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
  const wsUrl = `${protocol}//${nodeUrl.replace(/^https?:\/\//, '')}/ws`;

  const socket = new WebSocket(wsUrl);

  socket.onopen = () => {
    console.log('WebSocket connection established');
    // Subscribe to channels
    socket.send(
      JSON.stringify({
        action: 'subscribe',
        channels: ['blocks', 'transactions', 'governance'],
      })
    );
  };

  socket.onmessage = event => {
    const data = JSON.parse(event.data);
    handleWebSocketEvent(data);
  };

  socket.onerror = error => {
    console.error('WebSocket error:', error);
  };

  socket.onclose = () => {
    console.log('WebSocket connection closed');
    // Implement reconnection logic
    setTimeout(initializeWebSocket, 5000);
  };

  return socket;
}

// Handle incoming WebSocket events
function handleWebSocketEvent(data) {
  switch (data.channel) {
    case 'blockchain':
      if (data.event === 'BLOCK_CREATED') {
        updateBlockchainState(data.data);
      } else if (data.event === 'TRANSACTION_CREATED') {
        updateTransactionList(data.data.transaction);
      } else if (data.event === 'STATE_UPDATE') {
        updateDashboardMetrics(data.data.state);
      }
      break;

    case 'governance':
      if (data.event === 'PROPOSAL_CREATED') {
        updateGovernanceProposals(data.data.proposal);
      } else if (data.event === 'VOTE_CAST') {
        updateVoteTally(data.data);
      }
      break;

    // Handle other channels
  }
}
```

### GraphQL API Integration

For complex data queries, WrfView uses Wrfcoin Core's GraphQL endpoint:

```javascript
// Example GraphQL query
async function fetchUserDashboardData(userId) {
  const query = `
    query GetUserDashboard($userId: ID!) {
      user(id: $userId) {
        wallets {
          address
          balance
          transactions(limit: 5) {
            id
            amount
            timestamp
            type
          }
        }
        computationalNFTs {
          id
          name
          lastRun
          computationTime
        }
        miningStats {
          hashrate
          rewards24h
          activeSubscription {
            tier
            expiresAt
          }
        }
      }
    }
  `;

  const response = await fetch(`${nodeUrl}/graphql`, {
    method: 'POST',
    headers,
    body: JSON.stringify({
      query,
      variables: { userId },
    }),
  });

  if (!response.ok) throw new Error('GraphQL query failed');
  return await response.json();
}
```

## Weather Data API Integration

WrfView provides specialized interfaces for working with computational weather forecast NFTs:

```javascript
// Fetch weather forecast data
async function getForecastData(modelId, params) {
  const response = await fetch(`${nodeUrl}/api/weather/forecast/${modelId}`, {
    method: 'POST',
    headers,
    body: JSON.stringify(params),
  });
  if (!response.ok) throw new Error('Failed to fetch forecast data');
  return await response.json();
}

// Submit weather observation data
async function submitWeatherObservation(observationData) {
  const response = await fetch(`${nodeUrl}/api/weather/observations`, {
    method: 'POST',
    headers,
    body: JSON.stringify(observationData),
  });
  if (!response.ok) throw new Error('Failed to submit weather observation');
  return await response.json();
}
```

## Mining API Integration

```javascript
// Start mining with subscription
async function startMining(subscriptionTier, resourceAllocation) {
  const response = await fetch(`${nodeUrl}/api/mining/start`, {
    method: 'POST',
    headers,
    body: JSON.stringify({
      subscriptionTier,
      resourceAllocation,
      rewardAddress: currentWallet.address,
    }),
  });
  if (!response.ok) throw new Error('Failed to start mining');
  return await response.json();
}

// Get mining statistics
async function getMiningStats() {
  const response = await fetch(`${nodeUrl}/api/mining/stats`, { headers });
  if (!response.ok) throw new Error('Failed to fetch mining stats');
  return await response.json();
}
```

## Layer 2 Integration

WrfView interacts with Wrfcoin's Layer 2 scaling solutions:

```javascript
// Open state channel
async function openStateChannel(counterpartyAddress, initialDeposit) {
  const response = await fetch(`${nodeUrl}/api/layer2/channels`, {
    method: 'POST',
    headers,
    body: JSON.stringify({
      counterpartyAddress,
      initialDeposit,
    }),
  });
  if (!response.ok) throw new Error('Failed to open state channel');
  return await response.json();
}

// Execute off-chain transaction in state channel
async function executeChannelTransaction(channelId, txData) {
  const response = await fetch(`${nodeUrl}/api/layer2/channels/${channelId}/transactions`, {
    method: 'POST',
    headers,
    body: JSON.stringify(txData),
  });
  if (!response.ok) throw new Error('Failed to execute channel transaction');
  return await response.json();
}

// Close state channel
async function closeStateChannel(channelId) {
  const response = await fetch(`${nodeUrl}/api/layer2/channels/${channelId}/close`, {
    method: 'POST',
    headers,
  });
  if (!response.ok) throw new Error('Failed to close state channel');
  return await response.json();
}
```

## Error Handling

WrfView implements comprehensive error handling for API interactions:

```javascript
class WrfViewApiError extends Error {
  constructor(message, statusCode, apiResponseBody) {
    super(message);
    this.name = 'WrfViewApiError';
    this.statusCode = statusCode;
    this.apiResponse = apiResponseBody;
  }
}

async function safeApiCall(apiPromise, errorMessage) {
  try {
    return await apiPromise;
  } catch (error) {
    console.error(`API Error: ${errorMessage}`, error);

    if (error.response) {
      const statusCode = error.response.status;
      try {
        const apiResponseBody = await error.response.json();
        throw new WrfViewApiError(errorMessage, statusCode, apiResponseBody);
      } catch (e) {
        throw new WrfViewApiError(errorMessage, statusCode, { rawError: error.toString() });
      }
    }

    throw new WrfViewApiError(errorMessage, 0, { rawError: error.toString() });
  }
}
```

## API Rate Limiting and Caching

WrfView implements proper rate limiting and caching strategies:

```javascript
// API request with caching
const apiCache = new Map();

async function cachedApiCall(url, options, cacheTimeMs = 30000) {
  const cacheKey = `${url}:${JSON.stringify(options)}`;

  const cachedData = apiCache.get(cacheKey);
  if (cachedData && Date.now() - cachedData.timestamp < cacheTimeMs) {
    return cachedData.data;
  }

  const response = await fetch(url, options);
  if (!response.ok) throw new Error(`API request failed: ${response.status}`);

  const data = await response.json();

  apiCache.set(cacheKey, {
    timestamp: Date.now(),
    data,
  });

  return data;
}
```

## Conclusion

WrfView's integration with Wrfcoin Core APIs provides a comprehensive interface for interacting with
the blockchain. By following these patterns, developers can extend WrfView's functionality or build
their own applications on the Wrfcoin ecosystem.
