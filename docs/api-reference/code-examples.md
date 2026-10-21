# WRFCoin API Code Examples

This document provides comprehensive code examples for integrating with the WRFCoin platform APIs in
multiple programming languages.

## Table of Contents

- [Authentication](#authentication)
- [Blockchain Operations](#blockchain-operations)
- [Carbon Credit Management](#carbon-credit-management)
- [Weather Data Integration](#weather-data-integration)
- [Compute Network Operations](#compute-network-operations)
- [Wallet Management](#wallet-management)
- [Error Handling](#error-handling)

## Authentication

### API Key Authentication

#### JavaScript/TypeScript

```typescript
import axios from 'axios';

class WRFCoinAPI {
  private baseURL: string;
  private apiKey?: string;
  private privateKey?: string;

  constructor(baseURL: string, apiKey?: string) {
    this.baseURL = baseURL;
    this.apiKey = apiKey;
  }

  private getHeaders() {
    const headers: Record<string, string> = {
      'Content-Type': 'application/json',
      'X-API-Client': 'WRFCoin-SDK-JS',
    };

    if (this.apiKey) {
      headers['X-API-Key'] = this.apiKey;
    }

    return headers;
  }

  async request(method: string, endpoint: string, data?: any) {
    try {
      const response = await axios({
        method,
        url: `${this.baseURL}${endpoint}`,
        headers: this.getHeaders(),
        data,
      });

      return response.data;
    } catch (error) {
      throw this.handleError(error);
    }
  }

  private handleError(error: any) {
    if (error.response) {
      return new Error(`API Error: ${error.response.data.error || error.response.statusText}`);
    }
    return error;
  }
}

// Usage
const api = new WRFCoinAPI('https://testnet-api.wrfcoin.org', 'your-api-key');
```

#### Python

```python
import requests
import json
from typing import Optional, Dict, Any

class WRFCoinAPI:
    def __init__(self, base_url: str, api_key: Optional[str] = None):
        self.base_url = base_url
        self.api_key = api_key
        self.session = requests.Session()

    def _get_headers(self) -> Dict[str, str]:
        headers = {
            'Content-Type': 'application/json',
            'X-API-Client': 'WRFCoin-SDK-Python'
        }

        if self.api_key:
            headers['X-API-Key'] = self.api_key

        return headers

    def request(self, method: str, endpoint: str, data: Optional[Dict] = None) -> Dict[str, Any]:
        url = f"{self.base_url}{endpoint}"

        try:
            response = self.session.request(
                method=method,
                url=url,
                headers=self._get_headers(),
                json=data
            )
            response.raise_for_status()
            return response.json()

        except requests.exceptions.RequestException as e:
            raise Exception(f"API Error: {str(e)}")

# Usage
api = WRFCoinAPI('https://testnet-api.wrfcoin.org', 'your-api-key')
```

#### Go

```go
package wrfcoin

import (
    "bytes"
    "encoding/json"
    "fmt"
    "io"
    "net/http"
    "time"
)

type Client struct {
    BaseURL    string
    APIKey     string
    HTTPClient *http.Client
}

func NewClient(baseURL, apiKey string) *Client {
    return &Client{
        BaseURL: baseURL,
        APIKey:  apiKey,
        HTTPClient: &http.Client{
            Timeout: 30 * time.Second,
        },
    }
}

func (c *Client) Request(method, endpoint string, data interface{}) (map[string]interface{}, error) {
    var body io.Reader

    if data != nil {
        jsonData, err := json.Marshal(data)
        if err != nil {
            return nil, fmt.Errorf("failed to marshal request data: %v", err)
        }
        body = bytes.NewBuffer(jsonData)
    }

    req, err := http.NewRequest(method, c.BaseURL+endpoint, body)
    if err != nil {
        return nil, fmt.Errorf("failed to create request: %v", err)
    }

    req.Header.Set("Content-Type", "application/json")
    req.Header.Set("X-API-Client", "WRFCoin-SDK-Go")

    if c.APIKey != "" {
        req.Header.Set("X-API-Key", c.APIKey)
    }

    resp, err := c.HTTPClient.Do(req)
    if err != nil {
        return nil, fmt.Errorf("request failed: %v", err)
    }
    defer resp.Body.Close()

    var result map[string]interface{}
    if err := json.NewDecoder(resp.Body).Decode(&result); err != nil {
        return nil, fmt.Errorf("failed to decode response: %v", err)
    }

    if resp.StatusCode >= 400 {
        return nil, fmt.Errorf("API error: %v", result["error"])
    }

    return result, nil
}

// Usage
client := NewClient("https://testnet-api.wrfcoin.org", "your-api-key")
```

## Blockchain Operations

### Get Dashboard Statistics

#### JavaScript/TypeScript

```typescript
async function getDashboardStats() {
  try {
    const stats = await api.request('GET', '/api/dashboard/stats');

    console.log('Network Statistics:');
    console.log(`Total Nodes: ${stats.data.totalNodes}`);
    console.log(`Active Validators: ${stats.data.activeValidators}`);
    console.log(`Network Hashrate: ${stats.data.networkHashrate}`);
    console.log(`Total Supply: ${stats.data.totalSupply}`);
    console.log(`Market Cap: ${stats.data.marketCap}`);

    return stats.data;
  } catch (error) {
    console.error('Failed to fetch dashboard stats:', error);
    throw error;
  }
}

// Usage
getDashboardStats().then(stats => {
  // Process statistics
});
```

#### Python

```python
async def get_dashboard_stats(api: WRFCoinAPI):
    """Fetch comprehensive dashboard statistics"""
    try:
        response = api.request('GET', '/api/dashboard/stats')
        stats = response['data']

        print(f"Network Statistics:")
        print(f"Total Nodes: {stats['totalNodes']}")
        print(f"Active Validators: {stats['activeValidators']}")
        print(f"Network Hashrate: {stats['networkHashrate']}")
        print(f"Total Supply: {stats['totalSupply']}")
        print(f"Market Cap: {stats['marketCap']}")

        return stats

    except Exception as e:
        print(f"Failed to fetch dashboard stats: {e}")
        raise

# Usage
stats = get_dashboard_stats(api)
```

### Create Transaction

#### JavaScript/TypeScript

```typescript
import { ethers } from 'ethers';

interface CreateTransactionRequest {
  to: string;
  value: string;
  gasPrice?: string;
  gasLimit?: string;
  data?: string;
  privateKey: string;
}

async function createTransaction(request: CreateTransactionRequest) {
  try {
    // Validate Ethereum address
    if (!ethers.utils.isAddress(request.to)) {
      throw new Error('Invalid recipient address');
    }

    // Convert value to Wei if needed
    const valueInWei = ethers.utils.parseEther(request.value).toString();

    const transaction = await api.request('POST', '/api/transactions', {
      to: request.to,
      value: valueInWei,
      gasPrice: request.gasPrice || '20000000000', // 20 Gwei
      gasLimit: request.gasLimit || '21000',
      data: request.data || '0x',
      privateKey: request.privateKey,
    });

    console.log('Transaction created successfully:');
    console.log(`Transaction Hash: ${transaction.data.txHash}`);
    console.log(`Block Height: ${transaction.data.blockHeight}`);
    console.log(`Gas Used: ${transaction.data.gasUsed}`);

    return transaction.data;
  } catch (error) {
    console.error('Failed to create transaction:', error);
    throw error;
  }
}

// Usage
const txRequest: CreateTransactionRequest = {
  to: '0x742fD4dDf4529aFa7D24e5f47dFb5ea42e63e7dD',
  value: '1.0', // 1 WRF token
  privateKey: process.env.PRIVATE_KEY!,
};

createTransaction(txRequest);
```

#### Python

```python
from eth_utils import is_address
from web3 import Web3

def create_transaction(api: WRFCoinAPI, to: str, value: str, private_key: str,
                      gas_price: str = "20000000000", gas_limit: str = "21000"):
    """Create a new blockchain transaction"""

    # Validate Ethereum address
    if not is_address(to):
        raise ValueError("Invalid recipient address")

    # Convert value to Wei
    value_wei = Web3.toWei(value, 'ether')

    try:
        response = api.request('POST', '/api/transactions', {
            'to': to,
            'value': str(value_wei),
            'gasPrice': gas_price,
            'gasLimit': gas_limit,
            'data': '0x',
            'privateKey': private_key
        })

        transaction = response['data']

        print(f"Transaction created successfully:")
        print(f"Transaction Hash: {transaction['txHash']}")
        print(f"Block Height: {transaction['blockHeight']}")
        print(f"Gas Used: {transaction['gasUsed']}")

        return transaction

    except Exception as e:
        print(f"Failed to create transaction: {e}")
        raise

# Usage
tx = create_transaction(
    api=api,
    to='0x742fD4dDf4529aFa7D24e5f47dFb5ea42e63e7dD',
    value='1.0',  # 1 WRF token
    private_key=os.getenv('PRIVATE_KEY')
)
```

## Carbon Credit Management

### Get Carbon Credit Batches

#### JavaScript/TypeScript

```typescript
interface CarbonCreditFilters {
  page?: number;
  limit?: number;
  projectType?: 'forestry' | 'renewable_energy' | 'methane_capture' | 'direct_air_capture';
  vintage?: number;
  registryStandard?: 'VCS' | 'GS' | 'CDM' | 'CAR';
}

async function getCarbonCreditBatches(filters: CarbonCreditFilters = {}) {
  try {
    const queryParams = new URLSearchParams();

    Object.entries(filters).forEach(([key, value]) => {
      if (value !== undefined) {
        queryParams.append(key, value.toString());
      }
    });

    const endpoint = `/api/carbon-credits/batches?${queryParams}`;
    const response = await api.request('GET', endpoint);

    console.log(`Found ${response.data.pagination.totalItems} carbon credit batches`);

    response.data.batches.forEach((batch: any, index: number) => {
      console.log(`\nBatch ${index + 1}:`);
      console.log(`  Token ID: ${batch.tokenId}`);
      console.log(`  Project: ${batch.projectId}`);
      console.log(`  Type: ${batch.projectType}`);
      console.log(`  Vintage: ${batch.vintage}`);
      console.log(`  Location: ${batch.location}`);
      console.log(`  Total Supply: ${batch.totalSupply} kg CO2e`);
      console.log(`  Available: ${batch.totalSupply - batch.retiredSupply} kg CO2e`);
    });

    return response.data;
  } catch (error) {
    console.error('Failed to fetch carbon credit batches:', error);
    throw error;
  }
}

// Usage
getCarbonCreditBatches({
  projectType: 'forestry',
  vintage: 2023,
  registryStandard: 'VCS',
  limit: 10,
});
```

#### Python

```python
from typing import Optional, Literal

def get_carbon_credit_batches(
    api: WRFCoinAPI,
    page: Optional[int] = None,
    limit: Optional[int] = None,
    project_type: Optional[Literal['forestry', 'renewable_energy', 'methane_capture', 'direct_air_capture']] = None,
    vintage: Optional[int] = None,
    registry_standard: Optional[Literal['VCS', 'GS', 'CDM', 'CAR']] = None
):
    """Fetch carbon credit batches with optional filtering"""

    params = {}

    if page is not None:
        params['page'] = page
    if limit is not None:
        params['limit'] = limit
    if project_type is not None:
        params['projectType'] = project_type
    if vintage is not None:
        params['vintage'] = vintage
    if registry_standard is not None:
        params['registryStandard'] = registry_standard

    query_string = '&'.join([f"{k}={v}" for k, v in params.items()])
    endpoint = f"/api/carbon-credits/batches?{query_string}" if query_string else "/api/carbon-credits/batches"

    try:
        response = api.request('GET', endpoint)
        data = response['data']

        print(f"Found {data['pagination']['totalItems']} carbon credit batches")

        for i, batch in enumerate(data['batches']):
            print(f"\nBatch {i + 1}:")
            print(f"  Token ID: {batch['tokenId']}")
            print(f"  Project: {batch['projectId']}")
            print(f"  Type: {batch['projectType']}")
            print(f"  Vintage: {batch['vintage']}")
            print(f"  Location: {batch['location']}")
            print(f"  Total Supply: {batch['totalSupply']} kg CO2e")
            print(f"  Available: {int(batch['totalSupply']) - int(batch['retiredSupply'])} kg CO2e")

        return data

    except Exception as e:
        print(f"Failed to fetch carbon credit batches: {e}")
        raise

# Usage
batches = get_carbon_credit_batches(
    api=api,
    project_type='forestry',
    vintage=2023,
    registry_standard='VCS',
    limit=10
)
```

### Retire Carbon Credits

#### JavaScript/TypeScript

```typescript
interface RetireCarbonCreditsRequest {
  tokenId: number;
  amount: string;
  retirementReason: string;
  beneficiary?: string;
  privateKey: string;
}

async function retireCarbonCredits(request: RetireCarbonCreditsRequest) {
  try {
    // Validate input
    if (!request.retirementReason || request.retirementReason.length < 10) {
      throw new Error('Retirement reason must be at least 10 characters');
    }

    if (parseFloat(request.amount) <= 0) {
      throw new Error('Amount must be greater than zero');
    }

    const response = await api.request(
      'POST',
      `/api/carbon-credits/batches/${request.tokenId}/retire`,
      {
        amount: request.amount,
        retirementReason: request.retirementReason,
        beneficiary: request.beneficiary || 'Self',
        privateKey: request.privateKey,
      }
    );

    console.log('Carbon credits retired successfully:');
    console.log(`Transaction Hash: ${response.data.transactionHash}`);
    console.log(`Amount Retired: ${response.data.amountRetired} kg CO2e`);
    console.log(`Beneficiary: ${response.data.beneficiary}`);
    console.log(`Reason: ${response.data.retirementReason}`);

    return response.data;
  } catch (error) {
    console.error('Failed to retire carbon credits:', error);
    throw error;
  }
}

// Usage
retireCarbonCredits({
  tokenId: 1,
  amount: '100.5',
  retirementReason: 'Corporate carbon neutrality offset for Q4 2023 operations',
  beneficiary: 'Acme Corporation',
  privateKey: process.env.PRIVATE_KEY!,
});
```

## Weather Data Integration

### Register Weather Station

#### JavaScript/TypeScript

```typescript
interface WeatherStationConfig {
  stationId: string;
  name: string;
  location: string;
  coordinates: {
    latitude: number;
    longitude: number;
  };
  network: 'ambient_weather' | 'weatherflow' | 'davis' | 'netatmo';
  sensors: string[];
  apiKey: string;
}

async function registerWeatherStation(config: WeatherStationConfig) {
  try {
    // Validate coordinates
    if (config.coordinates.latitude < -90 || config.coordinates.latitude > 90) {
      throw new Error('Invalid latitude. Must be between -90 and 90');
    }

    if (config.coordinates.longitude < -180 || config.coordinates.longitude > 180) {
      throw new Error('Invalid longitude. Must be between -180 and 180');
    }

    const response = await api.request('POST', '/api/weather/stations', config);

    console.log('Weather station registered successfully:');
    console.log(`Station ID: ${response.data.stationId}`);
    console.log(`Name: ${response.data.name}`);
    console.log(`Location: ${response.data.location.name}`);
    console.log(`Network: ${response.data.network}`);
    console.log(`Status: ${response.data.status}`);
    console.log(`Sensors: ${response.data.sensors.join(', ')}`);

    return response.data;
  } catch (error) {
    console.error('Failed to register weather station:', error);
    throw error;
  }
}

// Usage
registerWeatherStation({
  stationId: 'WS-NYC-001',
  name: 'Manhattan Weather Station',
  location: 'New York City, NY, USA',
  coordinates: {
    latitude: 40.7128,
    longitude: -74.006,
  },
  network: 'ambient_weather',
  sensors: ['temperature', 'humidity', 'pressure', 'wind_speed', 'rainfall'],
  apiKey: process.env.WEATHER_API_KEY!,
});
```

### Fetch Weather Data

#### JavaScript/TypeScript

```typescript
interface WeatherDataQuery {
  location: string;
  startDate?: string;
  endDate?: string;
  metrics?: string[];
}

async function getWeatherData(query: WeatherDataQuery) {
  try {
    const params = new URLSearchParams();
    params.append('location', query.location);

    if (query.startDate) {
      params.append('startDate', query.startDate);
    }

    if (query.endDate) {
      params.append('endDate', query.endDate);
    }

    if (query.metrics && query.metrics.length > 0) {
      params.append('metrics', query.metrics.join(','));
    }

    const response = await api.request('GET', `/api/weather/data?${params}`);

    console.log(`Retrieved ${response.data.length} weather readings`);

    response.data.forEach((reading: any, index: number) => {
      console.log(`\nReading ${index + 1}:`);
      console.log(`  Station: ${reading.stationId}`);
      console.log(`  Time: ${reading.timestamp}`);
      console.log(`  Temperature: ${reading.metrics.temperature}°C`);
      console.log(`  Humidity: ${reading.metrics.humidity}%`);
      console.log(`  Pressure: ${reading.metrics.pressure} hPa`);
      console.log(`  Confidence: ${reading.confidence}%`);
    });

    return response.data;
  } catch (error) {
    console.error('Failed to fetch weather data:', error);
    throw error;
  }
}

// Usage
getWeatherData({
  location: '40.7128,-74.0060',
  startDate: '2024-01-01T00:00:00Z',
  endDate: '2024-01-31T23:59:59Z',
  metrics: ['temperature', 'humidity', 'pressure', 'windSpeed'],
});
```

## Compute Network Operations

### Submit Compute Job

#### JavaScript/TypeScript

```typescript
interface ComputeJobRequest {
  type: 'weather_model' | 'ml_training' | 'data_processing' | 'nft_computation';
  priority?: number;
  inputData: object;
  computeResources: {
    cpu: number;
    memory: number;
    gpu?: boolean;
  };
}

async function submitComputeJob(jobRequest: ComputeJobRequest) {
  try {
    // Validate compute resources
    if (jobRequest.computeResources.cpu < 0.1 || jobRequest.computeResources.cpu > 32) {
      throw new Error('CPU allocation must be between 0.1 and 32 cores');
    }

    if (jobRequest.computeResources.memory < 0.5 || jobRequest.computeResources.memory > 64) {
      throw new Error('Memory allocation must be between 0.5 and 64 GB');
    }

    const response = await api.request('POST', '/api/compute/jobs', jobRequest);

    console.log('Compute job submitted successfully:');
    console.log(`Job ID: ${response.data.jobId}`);
    console.log(`Type: ${response.data.type}`);
    console.log(`Status: ${response.data.status}`);
    console.log(`Priority: ${response.data.priority}`);
    console.log(`Estimated Duration: ${response.data.estimatedDuration}s`);

    // Poll for job completion
    pollJobStatus(response.data.jobId);

    return response.data;
  } catch (error) {
    console.error('Failed to submit compute job:', error);
    throw error;
  }
}

async function pollJobStatus(jobId: string) {
  const poll = async () => {
    try {
      const jobs = await api.request('GET', `/api/compute/jobs?jobId=${jobId}`);
      const job = jobs.data.jobs.find((j: any) => j.jobId === jobId);

      if (!job) {
        console.log('Job not found');
        return;
      }

      console.log(`Job Status: ${job.status}`);

      if (job.status === 'completed') {
        console.log('✅ Compute job completed successfully!');
        console.log(`Result: ${JSON.stringify(job.outputData, null, 2)}`);
        return;
      }

      if (job.status === 'failed') {
        console.log('❌ Compute job failed');
        console.log(`Error: ${job.error}`);
        return;
      }

      if (job.status === 'running' || job.status === 'pending') {
        setTimeout(poll, 5000); // Poll every 5 seconds
      }
    } catch (error) {
      console.error('Failed to check job status:', error);
    }
  };

  poll();
}

// Usage
submitComputeJob({
  type: 'weather_model',
  priority: 7,
  inputData: {
    modelType: 'GFS',
    region: 'north_america',
    resolution: '0.25_degree',
    forecastHours: 72,
  },
  computeResources: {
    cpu: 4,
    memory: 8,
    gpu: true,
  },
});
```

## Wallet Management

### Create Wallet

#### JavaScript/TypeScript

```typescript
import { ethers } from 'ethers';

interface CreateWalletRequest {
  entropy?: string;
  derivationPath?: string;
}

async function createWallet(request: CreateWalletRequest = {}) {
  try {
    const response = await api.request('POST', '/wallet/create', request);

    console.log('Wallet created successfully:');
    console.log(`Address: ${response.data.address}`);
    console.log(`Derivation Path: ${response.data.derivationPath}`);
    console.log(`HD Wallet: ${response.data.isHDWallet}`);

    // ⚠️ IMPORTANT: Store mnemonic securely
    console.log('⚠️  STORE THIS MNEMONIC SECURELY:');
    console.log(`Mnemonic: ${response.data.mnemonic}`);

    // Verify the wallet
    const wallet = ethers.Wallet.fromMnemonic(response.data.mnemonic, response.data.derivationPath);
    console.log(
      `✅ Verification: ${wallet.address === response.data.address ? 'PASSED' : 'FAILED'}`
    );

    return response.data;
  } catch (error) {
    console.error('Failed to create wallet:', error);
    throw error;
  }
}

// Usage
createWallet({
  derivationPath: "m/44'/60'/0'/0/0", // Ethereum standard path
});
```

### Get Wallet Balance

#### JavaScript/TypeScript

```typescript
async function getWalletBalance(address: string) {
  try {
    if (!ethers.utils.isAddress(address)) {
      throw new Error('Invalid wallet address');
    }

    const response = await api.request('GET', `/wallet/${address}/balance`);

    console.log(`Wallet Balance for ${address}:`);
    console.log(`Native Balance: ${ethers.utils.formatEther(response.data.balance)} WRF`);

    if (response.data.tokens.length > 0) {
      console.log('\nToken Balances:');
      response.data.tokens.forEach((token: any) => {
        const balance = ethers.utils.formatUnits(token.balance, token.decimals);
        console.log(`  ${token.symbol}: ${balance} ${token.name}`);
      });
    }

    if (response.data.nfts.length > 0) {
      console.log(`\nNFTs: ${response.data.nfts.length} items`);
      response.data.nfts.forEach((nft: any, index: number) => {
        console.log(`  ${index + 1}. ${nft.name} (Token ID: ${nft.tokenId})`);
      });
    }

    return response.data;
  } catch (error) {
    console.error('Failed to get wallet balance:', error);
    throw error;
  }
}

// Usage
getWalletBalance('0x742fD4dDf4529aFa7D24e5f47dFb5ea42e63e7dD');
```

## Error Handling

### Comprehensive Error Handling

#### JavaScript/TypeScript

```typescript
class WRFCoinAPIError extends Error {
  public code: string;
  public statusCode: number;
  public details?: any;

  constructor(message: string, code: string, statusCode: number, details?: any) {
    super(message);
    this.name = 'WRFCoinAPIError';
    this.code = code;
    this.statusCode = statusCode;
    this.details = details;
  }
}

function handleAPIError(error: any): WRFCoinAPIError {
  if (error.response) {
    const { status, data } = error.response;

    switch (status) {
      case 400:
        return new WRFCoinAPIError(data.error || 'Bad Request', 'BAD_REQUEST', 400, data);

      case 401:
        return new WRFCoinAPIError(
          'Unauthorized - check your API key or authentication',
          'UNAUTHORIZED',
          401
        );

      case 403:
        return new WRFCoinAPIError('Forbidden - insufficient permissions', 'FORBIDDEN', 403);

      case 404:
        return new WRFCoinAPIError('Resource not found', 'NOT_FOUND', 404);

      case 429:
        return new WRFCoinAPIError(
          'Rate limit exceeded - please wait before retrying',
          'RATE_LIMITED',
          429,
          { retryAfter: error.response.headers['retry-after'] }
        );

      case 500:
        return new WRFCoinAPIError(
          'Internal server error - please try again later',
          'INTERNAL_ERROR',
          500
        );

      default:
        return new WRFCoinAPIError(data.error || 'Unknown API error', 'UNKNOWN', status, data);
    }
  }

  if (error.code === 'ECONNREFUSED') {
    return new WRFCoinAPIError(
      'Cannot connect to API server - check your network connection',
      'CONNECTION_REFUSED',
      0
    );
  }

  if (error.code === 'ENOTFOUND') {
    return new WRFCoinAPIError('API server not found - check the base URL', 'HOST_NOT_FOUND', 0);
  }

  return new WRFCoinAPIError(error.message || 'Unknown error', 'UNKNOWN', 0);
}

// Usage with retry logic
async function apiRequestWithRetry(
  apiCall: () => Promise<any>,
  maxRetries: number = 3,
  backoffMs: number = 1000
): Promise<any> {
  let lastError: Error;

  for (let attempt = 1; attempt <= maxRetries; attempt++) {
    try {
      return await apiCall();
    } catch (error) {
      lastError = handleAPIError(error);

      // Don't retry on client errors (4xx)
      if (lastError.statusCode >= 400 && lastError.statusCode < 500) {
        throw lastError;
      }

      if (attempt === maxRetries) {
        break;
      }

      // Exponential backoff
      const delay = backoffMs * Math.pow(2, attempt - 1);
      console.log(`Attempt ${attempt} failed, retrying in ${delay}ms...`);
      await new Promise(resolve => setTimeout(resolve, delay));
    }
  }

  throw lastError!;
}

// Usage example
apiRequestWithRetry(
  async () => {
    return await getDashboardStats();
  },
  3,
  1000
)
  .then(stats => {
    console.log('Dashboard stats:', stats);
  })
  .catch(error => {
    console.error('Failed after retries:', error.message);
  });
```

## Best Practices

### Security Guidelines

1. **Never expose private keys in client-side code**
2. **Use environment variables for sensitive data**
3. **Implement proper input validation**
4. **Use HTTPS for all API calls**
5. **Implement rate limiting and backoff strategies**
6. **Validate all Ethereum addresses before API calls**
7. **Store mnemonics and private keys securely (encrypted)**

### Performance Optimization

1. **Use pagination for large datasets**
2. **Implement caching for frequently accessed data**
3. **Batch operations when possible**
4. **Use WebSockets for real-time updates**
5. **Implement connection pooling for high-volume applications**

### Development Workflow

1. **Start with testnet for development and testing**
2. **Use comprehensive error handling**
3. **Implement logging for debugging**
4. **Test all edge cases and error conditions**
5. **Monitor API usage and performance**

This completes the comprehensive code examples for the WRFCoin platform APIs. Each example includes
proper error handling, validation, and best practices for production use.
