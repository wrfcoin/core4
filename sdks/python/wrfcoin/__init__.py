"""
Wrfcoin Python SDK

A comprehensive Python SDK for interacting with the Wrfcoin blockchain ecosystem.
Provides convenient access to weather data validation, computational NFTs,
carbon credits, and governance.
"""

__version__ = "1.0.0"
__author__ = "Wrfcoin Team"
__email__ = "dev@wrfcoin.io"
__license__ = "MIT"

# Core client classes
from .client import WrfcoinSDK, GraphQLClient, SDKConfig, SDKStatus

# Service classes
from .services import (
    BlockchainService,
    WalletService,
    NFTService,
    WeatherService,
    GovernanceService
)

# Exception classes
from .exceptions import (
    WrfcoinError,
    NetworkError,
    AuthenticationError,
    ValidationError,
    ConfigurationError,
    TransactionError,
    InsufficientFundsError,
    ContractError,
    GovernanceError,
    WeatherDataError,
    NFTError,
    RateLimitError,
    TimeoutError,
    InvalidAddressError,
    InvalidAmountError,
    UnsupportedOperationError
)

# Convenience functions
def create_sdk(config=None, **kwargs):
    """Create a new WrfcoinSDK instance
    
    Args:
        config: SDKConfig instance or dict of configuration options
        **kwargs: Configuration options passed directly to SDKConfig
        
    Returns:
        WrfcoinSDK: Configured SDK instance
    """
    if config is None:
        config = kwargs
    return WrfcoinSDK(config)

def mainnet_sdk(**kwargs):
    """Create SDK instance configured for mainnet
    
    Args:
        **kwargs: Additional configuration options
        
    Returns:
        WrfcoinSDK: Mainnet SDK instance
    """
    return WrfcoinSDK.mainnet(**kwargs)

def testnet_sdk(**kwargs):
    """Create SDK instance configured for testnet
    
    Args:
        **kwargs: Additional configuration options
        
    Returns:
        WrfcoinSDK: Testnet SDK instance
    """
    return WrfcoinSDK.testnet(**kwargs)

def local_sdk(**kwargs):
    """Create SDK instance configured for local development
    
    Args:
        **kwargs: Additional configuration options
        
    Returns:
        WrfcoinSDK: Local SDK instance
    """
    return WrfcoinSDK.local(**kwargs)

# Export all public components
__all__ = [
    # Core classes
    'WrfcoinSDK',
    'GraphQLClient', 
    'SDKConfig',
    'SDKStatus',
    
    # Service classes
    'BlockchainService',
    'WalletService',
    'NFTService',
    'WeatherService',
    'GovernanceService',
    
    # Exception classes
    'WrfcoinError',
    'NetworkError',
    'AuthenticationError',
    'ValidationError',
    'ConfigurationError',
    'TransactionError',
    'InsufficientFundsError',
    'ContractError',
    'GovernanceError',
    'WeatherDataError',
    'NFTError',
    'RateLimitError',
    'TimeoutError',
    'InvalidAddressError',
    'InvalidAmountError',
    'UnsupportedOperationError',
    
    # Convenience functions
    'create_sdk',
    'mainnet_sdk',
    'testnet_sdk',
    'local_sdk',
]

# Package metadata
__description__ = "Python SDK for Wrfcoin blockchain ecosystem"
__url__ = "https://github.com/wrfcoin/wrfcoin-sdk-python"
__keywords__ = [
    "wrfcoin",
    "blockchain", 
    "weather",
    "nft",
    "carbon credits",
    "governance",
    "sdk"
]

# Version info tuple for programmatic access
VERSION_INFO = tuple(map(int, __version__.split('.')))

# Network configurations for quick access
NETWORKS = {
    "mainnet": {
        "endpoint": "https://api.wrfcoin.io/graphql",
        "ws_endpoint": "wss://api.wrfcoin.io/graphql",
        "chain_id": 1,
        "name": "Wrfcoin Mainnet"
    },
    "testnet": {
        "endpoint": "https://testnet-api.wrfcoin.io/graphql",
        "ws_endpoint": "wss://testnet-api.wrfcoin.io/graphql", 
        "chain_id": 2,
        "name": "Wrfcoin Testnet"
    },
    "local": {
        "endpoint": "http://localhost:4001/graphql",
        "ws_endpoint": "ws://localhost:4001/graphql",
        "chain_id": 1337,
        "name": "Local Development"
    }
}
