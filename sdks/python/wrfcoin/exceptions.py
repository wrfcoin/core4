"""
Wrfcoin Python SDK - Exception Classes

Custom exceptions for better error handling and debugging.
"""

from typing import Optional, Dict, Any


class WrfcoinError(Exception):
    """Base exception for all Wrfcoin SDK errors"""
    
    def __init__(self, message: str, details: Optional[Dict[str, Any]] = None):
        super().__init__(message)
        self.message = message
        self.details = details or {}
    
    def __str__(self) -> str:
        if self.details:
            return f"{self.message} - Details: {self.details}"
        return self.message


class NetworkError(WrfcoinError):
    """Network-related errors (timeouts, connection issues)"""
    pass


class AuthenticationError(WrfcoinError):
    """Authentication and authorization errors"""
    pass


class ValidationError(WrfcoinError):
    """Input validation and parameter errors"""
    pass


class ConfigurationError(WrfcoinError):
    """SDK configuration and setup errors"""
    pass


class TransactionError(WrfcoinError):
    """Blockchain transaction errors"""
    
    def __init__(self, message: str, tx_hash: Optional[str] = None, details: Optional[Dict[str, Any]] = None):
        super().__init__(message, details)
        self.tx_hash = tx_hash


class InsufficientFundsError(TransactionError):
    """Insufficient balance for transaction"""
    pass


class ContractError(WrfcoinError):
    """Smart contract interaction errors"""
    
    def __init__(self, message: str, contract_address: Optional[str] = None, details: Optional[Dict[str, Any]] = None):
        super().__init__(message, details)
        self.contract_address = contract_address


class GovernanceError(WrfcoinError):
    """Governance-related errors"""
    pass


class WeatherDataError(WrfcoinError):
    """Weather data validation and processing errors"""
    pass


class NFTError(WrfcoinError):
    """NFT-related errors"""
    
    def __init__(self, message: str, token_id: Optional[str] = None, details: Optional[Dict[str, Any]] = None):
        super().__init__(message, details)
        self.token_id = token_id


class RateLimitError(NetworkError):
    """API rate limit exceeded"""
    
    def __init__(self, message: str, retry_after: Optional[int] = None, details: Optional[Dict[str, Any]] = None):
        super().__init__(message, details)
        self.retry_after = retry_after


class TimeoutError(NetworkError):
    """Request timeout error"""
    pass


class InvalidAddressError(ValidationError):
    """Invalid blockchain address format"""
    pass


class InvalidAmountError(ValidationError):
    """Invalid amount or balance"""
    pass


class UnsupportedOperationError(WrfcoinError):
    """Operation not supported on current network"""
    pass


# Error type checking utilities
def is_retryable_error(error: Exception) -> bool:
    """Check if an error is retryable"""
    return isinstance(error, (NetworkError, TimeoutError, RateLimitError))


def is_authentication_error(error: Exception) -> bool:
    """Check if an error is authentication-related"""
    return isinstance(error, AuthenticationError)


def is_validation_error(error: Exception) -> bool:
    """Check if an error is validation-related"""
    return isinstance(error, ValidationError)


def get_error_code(error: Exception) -> str:
    """Get standardized error code for error type"""
    error_codes = {
        NetworkError: "NETWORK_ERROR",
        AuthenticationError: "AUTH_ERROR", 
        ValidationError: "VALIDATION_ERROR",
        ConfigurationError: "CONFIG_ERROR",
        TransactionError: "TRANSACTION_ERROR",
        InsufficientFundsError: "INSUFFICIENT_FUNDS",
        ContractError: "CONTRACT_ERROR",
        GovernanceError: "GOVERNANCE_ERROR",
        WeatherDataError: "WEATHER_DATA_ERROR",
        NFTError: "NFT_ERROR",
        RateLimitError: "RATE_LIMIT_ERROR",
        TimeoutError: "TIMEOUT_ERROR",
        InvalidAddressError: "INVALID_ADDRESS",
        InvalidAmountError: "INVALID_AMOUNT",
        UnsupportedOperationError: "UNSUPPORTED_OPERATION"
    }
    
    for error_type, code in error_codes.items():
        if isinstance(error, error_type):
            return code
    
    return "UNKNOWN_ERROR"
