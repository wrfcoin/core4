/**
 * Error classes for Wrfcoin SDK Extensions
 *
 * Comprehensive error handling with specific error types
 * for different failure scenarios.
 */

/**
 * Base error class for all Wrfcoin SDK errors
 */
export class WrfcoinError extends Error {
  public readonly code: string;
  public readonly details?: any;
  public readonly timestamp: Date;

  constructor(message: string, code = 'WRFCOIN_ERROR', details?: any) {
    super(message);
    this.name = 'WrfcoinError';
    this.code = code;
    this.details = details;
    this.timestamp = new Date();

    // Ensure proper prototype chain for instanceof checks
    Object.setPrototypeOf(this, WrfcoinError.prototype);
  }

  /**
   * Convert error to JSON for logging/debugging
   */
  toJSON(): Record<string, any> {
    return {
      name: this.name,
      message: this.message,
      code: this.code,
      details: this.details,
      timestamp: this.timestamp.toISOString(),
      stack: this.stack,
    };
  }
}

/**
 * Network-related errors (connection, timeout, etc.)
 */
export class NetworkError extends WrfcoinError {
  constructor(message: string, details?: any) {
    super(message, 'NETWORK_ERROR', details);
    this.name = 'NetworkError';
    Object.setPrototypeOf(this, NetworkError.prototype);
  }
}

/**
 * Authentication and authorization errors
 */
export class AuthenticationError extends WrfcoinError {
  constructor(message: string, details?: any) {
    super(message, 'AUTH_ERROR', details);
    this.name = 'AuthenticationError';
    Object.setPrototypeOf(this, AuthenticationError.prototype);
  }
}

/**
 * Validation errors for input data
 */
export class ValidationError extends WrfcoinError {
  public readonly field?: string;

  constructor(message: string, field?: string, details?: any) {
    super(message, 'VALIDATION_ERROR', details);
    this.name = 'ValidationError';
    this.field = field;
    Object.setPrototypeOf(this, ValidationError.prototype);
  }
}

/**
 * Blockchain-related errors (transaction failed, insufficient funds, etc.)
 */
export class BlockchainError extends WrfcoinError {
  public readonly transactionHash?: string;
  public readonly blockNumber?: number;

  constructor(
    message: string,
    transactionHash?: string,
    blockNumber?: number,
    details?: any
  ) {
    super(message, 'BLOCKCHAIN_ERROR', details);
    this.name = 'BlockchainError';
    this.transactionHash = transactionHash;
    this.blockNumber = blockNumber;
    Object.setPrototypeOf(this, BlockchainError.prototype);
  }
}

/**
 * Weather data related errors
 */
export class WeatherError extends WrfcoinError {
  public readonly stationId?: string;

  constructor(message: string, stationId?: string, details?: any) {
    super(message, 'WEATHER_ERROR', details);
    this.name = 'WeatherError';
    this.stationId = stationId;
    Object.setPrototypeOf(this, WeatherError.prototype);
  }
}

/**
 * Utility function to create appropriate error from response
 */
export function createErrorFromResponse(response: any): WrfcoinError {
  const { status, statusText, data } = response;
  const message = data?.error || data?.message || statusText || 'Unknown error';

  switch (status) {
    case 401:
    case 403:
      return new AuthenticationError(message);
    case 400:
      return new ValidationError(message);
    case 500:
    case 502:
    case 503:
    case 504:
      return new NetworkError(message);
    default:
      return new WrfcoinError(message, `HTTP_${status}`);
  }
}

/**
 * Type guard to check if error is a Wrfcoin SDK error
 */
export function isWrfcoinError(error: unknown): error is WrfcoinError {
  return error instanceof WrfcoinError;
}

/**
 * Type guard for specific error types
 */
export function isNetworkError(error: unknown): error is NetworkError {
  return error instanceof NetworkError;
}

export function isAuthenticationError(
  error: unknown
): error is AuthenticationError {
  return error instanceof AuthenticationError;
}

export function isValidationError(error: unknown): error is ValidationError {
  return error instanceof ValidationError;
}

export function isBlockchainError(error: unknown): error is BlockchainError {
  return error instanceof BlockchainError;
}

export function isWeatherError(error: unknown): error is WeatherError {
  return error instanceof WeatherError;
}
