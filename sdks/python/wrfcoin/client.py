"""
Wrfcoin Python SDK - Core Client Classes

Main SDK client and GraphQL communication handling.
"""

import asyncio
import json
import time
from typing import Any, Dict, List, Optional, Union, Callable
from dataclasses import dataclass, field
from datetime import datetime

import aiohttp
import websockets
from gql import Client, gql
from gql.transport.aiohttp import AIOHTTPTransport
from gql.transport.websockets import WebsocketsTransport

from .exceptions import (
    WrfcoinError,
    NetworkError,
    AuthenticationError,
    ValidationError,
    ConfigurationError
)


@dataclass
class SDKConfig:
    """Configuration for the Wrfcoin SDK"""
    endpoint: str
    ws_endpoint: Optional[str] = None
    api_key: Optional[str] = None
    network: str = "mainnet"
    timeout: int = 30
    retries: int = 3
    debug: bool = False
    auto_connect: bool = True
    cache_enabled: bool = True
    max_cache_size: int = 1000


@dataclass
class SDKStatus:
    """Current status of the SDK connection"""
    connected: bool
    authenticated: bool
    network: str
    block_height: Optional[int] = None
    last_sync: Optional[datetime] = None


class GraphQLClient:
    """
    GraphQL client for Wrfcoin API with authentication, error handling, and subscriptions.
    """
    
    def __init__(self, config: SDKConfig):
        self.config = config
        self._client: Optional[Client] = None
        self._ws_client: Optional[Client] = None
        self._session: Optional[aiohttp.ClientSession] = None
        self._is_connected = False
        
        # Initialize HTTP transport
        self._init_http_transport()
        
        # Initialize WebSocket transport if endpoint provided
        if config.ws_endpoint:
            self._init_ws_transport()
    
    def _init_http_transport(self) -> None:
        """Initialize HTTP transport for GraphQL queries"""
        headers = self._get_default_headers()
        
        transport = AIOHTTPTransport(
            url=self.config.endpoint,
            headers=headers,
            timeout=self.config.timeout
        )
        
        self._client = Client(transport=transport, fetch_schema_from_transport=False)
    
    def _init_ws_transport(self) -> None:
        """Initialize WebSocket transport for subscriptions"""
        if not self.config.ws_endpoint:
            return
            
        connection_params = {}
        if self.config.api_key:
            connection_params["Authorization"] = f"Bearer {self.config.api_key}"
        
        ws_transport = WebsocketsTransport(
            url=self.config.ws_endpoint,
            init_payload=connection_params
        )
        
        self._ws_client = Client(transport=ws_transport, fetch_schema_from_transport=False)
    
    def _get_default_headers(self) -> Dict[str, str]:
        """Get default headers for requests"""
        headers = {
            "Content-Type": "application/json",
            "User-Agent": "wrfcoin-python-sdk/1.0.0"
        }
        
        if self.config.api_key:
            headers["Authorization"] = f"Bearer {self.config.api_key}"
        
        return headers
    
    async def query(self, query: str, variables: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
        """Execute a GraphQL query"""
        if not self._client:
            raise WrfcoinError("GraphQL client not initialized")
        
        retries = self.config.retries
        last_error = None
        
        for attempt in range(retries + 1):
            try:
                if self.config.debug:
                    print(f"GraphQL Query (attempt {attempt + 1}): {query[:100]}...")
                
                document = gql(query)
                result = await self._client.execute_async(document, variable_values=variables)
                
                if self.config.debug:
                    print(f"GraphQL Response: {str(result)[:200]}...")
                
                return result
                
            except Exception as error:
                last_error = self._handle_error(error)
                
                if attempt < retries and self._should_retry(last_error):
                    delay = (2 ** attempt) * 1000  # Exponential backoff
                    await asyncio.sleep(delay / 1000.0)
                    continue
                
                raise last_error
        
        raise last_error or WrfcoinError("Query failed after retries")
    
    async def mutate(self, mutation: str, variables: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
        """Execute a GraphQL mutation"""
        return await self.query(mutation, variables)
    
    async def subscribe(
        self,
        subscription: str,
        variables: Optional[Dict[str, Any]] = None,
        on_data: Optional[Callable[[Dict[str, Any]], None]] = None,
        on_error: Optional[Callable[[Exception], None]] = None,
        on_complete: Optional[Callable[[], None]] = None
    ) -> Callable[[], None]:
        """Subscribe to GraphQL subscription"""
        if not self._ws_client:
            raise WrfcoinError("WebSocket client not initialized")
        
        document = gql(subscription)
        
        async def _subscription_handler():
            try:
                async for result in self._ws_client.subscribe_async(document, variable_values=variables):
                    if self.config.debug:
                        print(f"Subscription data: {str(result)[:200]}...")
                    
                    if on_data:
                        on_data(result)
                        
            except Exception as error:
                wrapped_error = self._handle_error(error)
                if on_error:
                    on_error(wrapped_error)
                raise wrapped_error
            finally:
                if on_complete:
                    on_complete()
        
        # Start subscription task
        task = asyncio.create_task(_subscription_handler())
        
        # Return unsubscribe function
        def unsubscribe():
            task.cancel()
        
        return unsubscribe
    
    def set_auth_token(self, token: str) -> None:
        """Set authentication token"""
        self.config.api_key = token
        
        # Reinitialize transports with new auth
        self._init_http_transport()
        if self.config.ws_endpoint:
            self._init_ws_transport()
    
    def clear_auth(self) -> None:
        """Clear authentication"""
        self.config.api_key = None
        
        # Reinitialize transports without auth
        self._init_http_transport()
        if self.config.ws_endpoint:
            self._init_ws_transport()
    
    def is_authenticated(self) -> bool:
        """Check if client is authenticated"""
        return bool(self.config.api_key)
    
    async def close(self) -> None:
        """Close all connections"""
        if self._client:
            await self._client.close_async()
        
        if self._ws_client:
            await self._ws_client.close_async()
        
        if self._session:
            await self._session.close()
    
    def _handle_error(self, error: Exception) -> WrfcoinError:
        """Handle and normalize errors"""
        if isinstance(error, WrfcoinError):
            return error
        
        error_message = str(error)
        
        # Check for authentication errors
        if "401" in error_message or "403" in error_message or "unauthorized" in error_message.lower():
            return AuthenticationError(f"Authentication failed: {error_message}")
        
        # Check for network errors
        if "timeout" in error_message.lower() or "connection" in error_message.lower():
            return NetworkError(f"Network error: {error_message}")
        
        # Check for validation errors
        if "400" in error_message or "invalid" in error_message.lower():
            return ValidationError(f"Validation error: {error_message}")
        
        return WrfcoinError(f"GraphQL error: {error_message}")
    
    def _should_retry(self, error: WrfcoinError) -> bool:
        """Determine if error is retryable"""
        return (
            isinstance(error, NetworkError) or
            "timeout" in error.message.lower() or
            "connection reset" in error.message.lower()
        )


class WrfcoinSDK:
    """
    Main Wrfcoin SDK client providing access to all blockchain functionality.
    """
    
    def __init__(self, config: Union[SDKConfig, Dict[str, Any]]):
        if isinstance(config, dict):
            self.config = SDKConfig(**config)
        else:
            self.config = config
        
        # Determine endpoints based on network
        self._setup_network_endpoints()
        
        # Initialize GraphQL client
        self.graphql = GraphQLClient(self.config)
        
        # Initialize services (will be imported lazily)
        self._blockchain = None
        self._wallet = None
        self._nft = None
        self._weather = None
        self._governance = None
        
        # Auto-connect if enabled
        if self.config.auto_connect:
            asyncio.create_task(self._auto_connect())
    
    def _setup_network_endpoints(self) -> None:
        """Setup network endpoints based on network type"""
        endpoints = self._get_network_endpoints(self.config.network)
        
        if not self.config.endpoint:
            self.config.endpoint = endpoints["endpoint"]
        
        if not self.config.ws_endpoint:
            self.config.ws_endpoint = endpoints["ws_endpoint"]
    
    def _get_network_endpoints(self, network: str) -> Dict[str, str]:
        """Get network endpoints based on network type"""
        endpoints_map = {
            "mainnet": {
                "endpoint": "https://api.wrfcoin.io/graphql",
                "ws_endpoint": "wss://api.wrfcoin.io/graphql"
            },
            "testnet": {
                "endpoint": "https://testnet-api.wrfcoin.io/graphql", 
                "ws_endpoint": "wss://testnet-api.wrfcoin.io/graphql"
            },
            "local": {
                "endpoint": "http://localhost:4001/graphql",
                "ws_endpoint": "ws://localhost:4001/graphql"
            }
        }
        
        if network not in endpoints_map:
            raise ConfigurationError(f"Unsupported network: {network}")
        
        return endpoints_map[network]
    
    async def _auto_connect(self) -> None:
        """Auto-connect to network if enabled"""
        try:
            await self.connect()
            if self.config.debug:
                print(f"Auto-connected to {self.config.network} network")
        except Exception as error:
            if self.config.debug:
                print(f"Auto-connect failed: {error}")
    
    async def connect(self) -> None:
        """Connect to the Wrfcoin network"""
        try:
            # Test connection with a simple query
            from .services.blockchain import BlockchainService
            blockchain = BlockchainService(self.graphql)
            await blockchain.get_network_info()
            
            if self.config.debug:
                print(f"Connected to Wrfcoin {self.config.network} network")
                
        except Exception as error:
            raise WrfcoinError(
                f"Failed to connect to {self.config.network} network: {error}"
            )
    
    async def disconnect(self) -> None:
        """Disconnect from the network"""
        await self.graphql.close()
        
        if self.config.debug:
            print("Disconnected from Wrfcoin network")
    
    def authenticate(self, token: str) -> None:
        """Authenticate with API key or token"""
        self.graphql.set_auth_token(token)
        
        if self.config.debug:
            print("Authentication token set")
    
    def unauthenticate(self) -> None:
        """Clear authentication"""
        self.graphql.clear_auth()
        
        if self.config.debug:
            print("Authentication cleared")
    
    async def get_status(self) -> SDKStatus:
        """Get current SDK status"""
        try:
            from .services.blockchain import BlockchainService
            blockchain = BlockchainService(self.graphql)
            network_info = await blockchain.get_network_info()
            
            return SDKStatus(
                connected=True,
                authenticated=self.graphql.is_authenticated(),
                network=self.config.network,
                block_height=network_info.get("block_height"),
                last_sync=datetime.now()
            )
        except Exception:
            return SDKStatus(
                connected=False,
                authenticated=self.graphql.is_authenticated(),
                network=self.config.network
            )
    
    async def is_ready(self) -> bool:
        """Check if SDK is connected and ready"""
        try:
            from .services.blockchain import BlockchainService
            blockchain = BlockchainService(self.graphql)
            await blockchain.get_network_info()
            return True
        except Exception:
            return False
    
    async def switch_network(self, network: str) -> None:
        """Switch to a different network"""
        if network == self.config.network:
            return  # Already on this network
        
        # Validate network
        try:
            self._get_network_endpoints(network)
        except ConfigurationError:
            raise
        
        # Update configuration
        old_network = self.config.network
        self.config.network = network
        self._setup_network_endpoints()
        
        # Reinitialize GraphQL client
        old_auth = self.config.api_key
        self.graphql = GraphQLClient(self.config)
        
        if old_auth:
            self.graphql.set_auth_token(old_auth)
        
        # Test connection to new network
        try:
            await self.connect()
            if self.config.debug:
                print(f"Switched from {old_network} to {network} network")
        except Exception as error:
            # Revert on failure
            self.config.network = old_network
            self._setup_network_endpoints()
            self.graphql = GraphQLClient(self.config)
            if old_auth:
                self.graphql.set_auth_token(old_auth)
            raise WrfcoinError(f"Failed to switch to {network} network: {error}")
    
    @property
    def blockchain(self):
        """Get blockchain service instance"""
        if self._blockchain is None:
            from .services.blockchain import BlockchainService
            self._blockchain = BlockchainService(self.graphql)
        return self._blockchain
    
    @property  
    def wallet(self):
        """Get wallet service instance"""
        if self._wallet is None:
            from .services.wallet import WalletService
            self._wallet = WalletService(self.graphql)
        return self._wallet
    
    @property
    def nft(self):
        """Get NFT service instance"""
        if self._nft is None:
            from .services.nft import NFTService
            self._nft = NFTService(self.graphql)
        return self._nft
    
    @property
    def weather(self):
        """Get weather service instance"""
        if self._weather is None:
            from .services.weather import WeatherService
            self._weather = WeatherService(self.graphql)
        return self._weather
    
    @property
    def governance(self):
        """Get governance service instance"""
        if self._governance is None:
            from .services.governance import GovernanceService
            self._governance = GovernanceService(self.graphql)
        return self._governance
    
    def set_debug_mode(self, enabled: bool) -> None:
        """Enable or disable debug mode"""
        self.config.debug = enabled
        
        if enabled:
            print("Debug mode enabled")
    
    @classmethod
    def create(cls, config: Union[SDKConfig, Dict[str, Any]]) -> "WrfcoinSDK":
        """Create a new SDK instance"""
        return cls(config)
    
    @classmethod
    def mainnet(cls, **kwargs) -> "WrfcoinSDK":
        """Create SDK instance for mainnet"""
        config = SDKConfig(network="mainnet", endpoint="", **kwargs)
        return cls(config)
    
    @classmethod
    def testnet(cls, **kwargs) -> "WrfcoinSDK":
        """Create SDK instance for testnet"""
        config = SDKConfig(network="testnet", endpoint="", **kwargs)
        return cls(config)
    
    @classmethod
    def local(cls, **kwargs) -> "WrfcoinSDK":
        """Create SDK instance for local development"""
        config = SDKConfig(network="local", endpoint="", **kwargs)
        return cls(config)
    
    @property
    def version(self) -> str:
        """Get SDK version"""
        return "1.0.0"
    
    @property
    def configuration(self) -> SDKConfig:
        """Get SDK configuration"""
        return self.config
