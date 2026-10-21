# Wrfcoin Python SDK

A comprehensive Python SDK for interacting with the Wrfcoin blockchain ecosystem. Provides
convenient access to weather data validation, computational NFTs, carbon credits, and governance.

## Features

- **Weather Data Integration**: Submit and validate weather observations, run computational models
- **NFT Operations**: Mint, transfer, and trade computational and carbon credit NFTs
- **Governance**: Create proposals, vote, delegate voting power
- **Wallet Management**: Send transactions, manage token balances, view transaction history
- **Real-time Subscriptions**: WebSocket support for live data feeds
- **Type Safety**: Full type hints and validation for better development experience
- **Error Handling**: Comprehensive exception hierarchy with detailed error information

## Installation

```bash
pip install wrfcoin
```

For development with optional dependencies:

```bash
pip install wrfcoin[dev,validation,cli]
```

## Quick Start

### Basic Usage

```python
import asyncio
from wrfcoin import WrfcoinSDK

async def main():
    # Create SDK instance for mainnet
    sdk = WrfcoinSDK.mainnet()

    # Get network information
    network_info = await sdk.blockchain.get_network_info()
    print(f"Block height: {network_info['blockNumber']}")

    # Get account balance
    balance = await sdk.wallet.get_balance("0x742d35Cc6634C0532925a3b8D3B9E0C0Da8E4Baf")
    print(f"Balance: {balance['balance']} WRF")

    await sdk.disconnect()

asyncio.run(main())
```

### Configuration

```python
from wrfcoin import WrfcoinSDK, SDKConfig

# Custom configuration
config = SDKConfig(
    endpoint="https://api.wrfcoin.io/graphql",
    ws_endpoint="wss://api.wrfcoin.io/graphql",
    api_key="your-api-key",
    network="mainnet",
    timeout=30,
    debug=True
)

sdk = WrfcoinSDK(config)
```

### Network-specific Instances

```python
# Mainnet
mainnet_sdk = WrfcoinSDK.mainnet(api_key="your-key")

# Testnet
testnet_sdk = WrfcoinSDK.testnet(debug=True)

# Local development
local_sdk = WrfcoinSDK.local(endpoint="http://localhost:4001/graphql")
```

## Services

### Blockchain Service

```python
# Get latest block
block = await sdk.blockchain.get_block()

# Get transaction details
tx = await sdk.blockchain.get_transaction("0x1234...")

# Estimate gas for transaction
gas_estimate = await sdk.blockchain.estimate_gas({
    "from": "0x742d35Cc6634C0532925a3b8D3B9E0C0Da8E4Baf",
    "to": "0x8ba1f109551bD432803012645Hac136c45cb2274",
    "value": "1000000000000000000"  # 1 WRF
})
```

### Wallet Service

```python
# Send transaction
result = await sdk.wallet.send_transaction({
    "from": "0x742d35Cc6634C0532925a3b8D3B9E0C0Da8E4Baf",
    "to": "0x8ba1f109551bD432803012645Hac136c45cb2274",
    "value": "1000000000000000000",  # 1 WRF in wei
    "gasPrice": "20000000000",
    "gasLimit": "21000"
})

# Get transaction history
history = await sdk.wallet.get_transaction_history(
    "0x742d35Cc6634C0532925a3b8D3B9E0C0Da8E4Baf",
    limit=10
)

# Get portfolio overview
portfolio = await sdk.wallet.get_portfolio("0x742d35Cc6634C0532925a3b8D3B9E0C0Da8E4Baf")
```

### Weather Service

```python
# Get weather data
weather = await sdk.weather.get_weather_data(
    location="New York, NY",
    start_date="2024-01-01",
    end_date="2024-01-31"
)

# Submit weather observation
observation = await sdk.weather.submit_weather_observation(
    observer_address="0x742d35Cc6634C0532925a3b8D3B9E0C0Da8E4Baf",
    location={
        "name": "Central Park",
        "latitude": 40.7829,
        "longitude": -73.9654
    },
    observation={
        "timestamp": "2024-01-15T12:00:00Z",
        "temperature": 15.5,
        "humidity": 65,
        "pressure": 1013.25,
        "windSpeed": 10.5
    }
)

# Run weather computation
computation = await sdk.weather.run_weather_computation(
    requester_address="0x742d35Cc6634C0532925a3b8D3B9E0C0Da8E4Baf",
    model_id="wrf-4km-hourly",
    input_parameters={
        "location": "40.7829,-73.9654",
        "forecast_hours": 72,
        "resolution": "1km"
    }
)
```

### NFT Service

```python
# Get NFT details
nft = await sdk.nft.get_nft("12345", collection_address="0x...")

# Mint computational NFT
computational_nft = await sdk.nft.mint_computational_nft(
    to_address="0x742d35Cc6634C0532925a3b8D3B9E0C0Da8E4Baf",
    model_id="weather-prediction-v2",
    input_data={
        "location": "40.7829,-73.9654",
        "date": "2024-01-15",
        "parameters": {"resolution": "1km"}
    },
    metadata={
        "name": "Weather Prediction NFT",
        "description": "72-hour weather forecast for NYC",
        "image": "ipfs://..."
    }
)

# List NFT for sale
listing = await sdk.nft.list_nft_for_sale(
    owner_address="0x742d35Cc6634C0532925a3b8D3B9E0C0Da8E4Baf",
    token_id="12345",
    price="100",
    currency="WRF"
)
```

### Governance Service

```python
# Get proposal details
proposal = await sdk.governance.get_proposal("prop-123")

# Create new proposal
new_proposal = await sdk.governance.create_proposal(
    proposer_address="0x742d35Cc6634C0532925a3b8D3B9E0C0Da8E4Baf",
    title="Increase Weather Validator Rewards",
    description="Proposal to increase rewards for weather data validators...",
    category="economic",
    actions=[{
        "target": "0x...",
        "data": "0x...",
        "value": "0"
    }]
)

# Vote on proposal
vote = await sdk.governance.vote_on_proposal(
    voter_address="0x742d35Cc6634C0532925a3b8D3B9E0C0Da8E4Baf",
    proposal_id="prop-123",
    choice=1,  # 0=against, 1=for, 2=abstain
    reason="This will improve data quality incentives"
)
```

## Real-time Subscriptions

```python
# Subscribe to new blocks
def on_new_block(block_data):
    print(f"New block: {block_data['number']}")

unsubscribe = await sdk.blockchain.subscribe_to_blocks(on_new_block)

# Subscribe to balance changes
def on_balance_change(balance_data):
    print(f"Balance changed: {balance_data['balance']}")

unsubscribe_balance = await sdk.wallet.subscribe_to_balance_changes(
    "0x742d35Cc6634C0532925a3b8D3B9E0C0Da8E4Baf",
    on_balance_change
)

# Unsubscribe when done
unsubscribe()
unsubscribe_balance()
```

## Error Handling

```python
from wrfcoin import (
    WrfcoinError,
    NetworkError,
    AuthenticationError,
    ValidationError,
    InsufficientFundsError
)

try:
    result = await sdk.wallet.send_transaction(tx_data)
except InsufficientFundsError as e:
    print(f"Insufficient funds: {e.message}")
except ValidationError as e:
    print(f"Invalid transaction data: {e.message}")
except NetworkError as e:
    print(f"Network error: {e.message}")
except WrfcoinError as e:
    print(f"SDK error: {e.message}")
```

## Authentication

```python
# Set API key
sdk.authenticate("your-api-key-here")

# Check authentication status
if sdk.graphql.is_authenticated():
    print("Authenticated successfully")

# Clear authentication
sdk.unauthenticate()
```

## Development

### Setting up Development Environment

```bash
git clone https://github.com/wrfcoin/wrfcoin-sdk-python
cd wrfcoin-sdk-python

# Create virtual environment
python -m venv venv
source venv/bin/activate  # On Windows: venv\Scripts\activate

# Install in development mode
pip install -e ".[dev]"
```

### Running Tests

```bash
# Run all tests
pytest

# Run with coverage
pytest --cov=wrfcoin --cov-report=html

# Run only unit tests
pytest -m unit

# Run only integration tests
pytest -m integration
```

### Code Formatting

```bash
# Format code
black wrfcoin/
isort wrfcoin/

# Lint code
flake8 wrfcoin/
mypy wrfcoin/
```

## API Reference

For detailed API documentation, visit: https://docs.wrfcoin.io/sdk/python

## Examples

See the `/examples` directory for complete usage examples:

- `basic_usage.py` - Basic SDK operations
- `weather_data.py` - Weather data submission and validation
- `nft_operations.py` - NFT minting and trading
- `governance.py` - DAO governance participation
- `real_time.py` - WebSocket subscriptions

## Contributing

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/amazing-feature`)
3. Commit your changes (`git commit -m 'Add amazing feature'`)
4. Push to the branch (`git push origin feature/amazing-feature`)
5. Open a Pull Request

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## Support

- Documentation: https://docs.wrfcoin.io
- Discord: https://discord.gg/wrfcoin
- GitHub Issues: https://github.com/wrfcoin/wrfcoin-sdk-python/issues
- Email: dev@wrfcoin.io
