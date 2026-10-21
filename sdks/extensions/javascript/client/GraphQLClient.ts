/**
 * GraphQL Client for Wrfcoin API
 *
 * Handles GraphQL communication with the Wrfcoin backend,
 * including authentication, error handling, and subscriptions.
 */

import {
  GraphQLClient as GQLClient,
  RequestDocument,
  Variables,
} from 'graphql-request';
import { Client, createClient } from 'graphql-ws';
import {
  WrfcoinError,
  NetworkError,
  AuthenticationError,
} from '../errors/index.js';

export interface GraphQLClientConfig {
  endpoint: string;
  wsEndpoint?: string;
  apiKey?: string;
  timeout?: number;
  retries?: number;
  debug?: boolean;
}

export interface RequestOptions {
  headers?: Record<string, string>;
  timeout?: number;
  retries?: number;
}

export interface SubscriptionOptions {
  onData?: (data: any) => void;
  onError?: (error: Error) => void;
  onComplete?: () => void;
}

export class GraphQLClient {
  private client: GQLClient;
  private wsClient?: Client;
  private config: Required<Omit<GraphQLClientConfig, 'apiKey'>> & {
    apiKey?: string;
  };

  constructor(config: GraphQLClientConfig) {
    this.config = {
      timeout: 30000,
      retries: 3,
      debug: false,
      ...config,
      wsEndpoint: config.wsEndpoint || config.endpoint.replace('http', 'ws'),
    };

    // Initialize HTTP client
    this.client = new GQLClient(this.config.endpoint, {
      headers: this.getDefaultHeaders(),
    });

    // Initialize WebSocket client if endpoint provided
    if (this.config.wsEndpoint) {
      this.initializeWSClient();
    }
  }

  /**
   * Execute a GraphQL query
   */
  async query<T = any>(
    document: RequestDocument,
    variables?: Variables,
    options: RequestOptions = {}
  ): Promise<T> {
    const headers = {
      ...this.getDefaultHeaders(),
      ...options.headers,
    };

    const retries = options.retries ?? this.config.retries;
    let lastError: Error | null = null;

    for (let attempt = 0; attempt <= retries; attempt++) {
      try {
        if (this.config.debug) {
          console.log('GraphQL Query:', { document, variables, attempt });
        }

        const result = await this.client.request<T>(
          document,
          variables,
          headers
        );

        if (this.config.debug) {
          console.log('GraphQL Response:', result);
        }

        return result;
      } catch (error) {
        lastError = this.handleError(error as any);

        if (attempt < retries && this.shouldRetry(lastError as WrfcoinError)) {
          const delay = Math.pow(2, attempt) * 1000; // Exponential backoff
          await this.delay(delay);
          continue;
        }

        throw lastError;
      }
    }

    throw lastError || new WrfcoinError('Query failed after retries');
  }

  /**
   * Execute a GraphQL mutation
   */
  async mutate<T = any>(
    document: RequestDocument,
    variables?: Variables,
    options: RequestOptions = {}
  ): Promise<T> {
    return this.query<T>(document, variables, options);
  }

  /**
   * Subscribe to GraphQL subscription
   */
  subscribe<T = any>(
    document: RequestDocument,
    variables?: Variables,
    options: SubscriptionOptions = {}
  ): () => void {
    if (!this.wsClient) {
      throw new WrfcoinError('WebSocket client not initialized');
    }

    const unsubscribe = this.wsClient.subscribe(
      {
        query: document.toString(),
        variables: variables as Record<string, unknown> | undefined,
      },
      {
        next: data => {
          if (this.config.debug) {
            console.log('Subscription data:', data);
          }
          options.onData?.(data.data);
        },
        error: error => {
          const wrappedError = this.handleError(error);
          if (this.config.debug) {
            console.error('Subscription error:', wrappedError);
          }
          options.onError?.(wrappedError);
        },
        complete: () => {
          if (this.config.debug) {
            console.log('Subscription completed');
          }
          options.onComplete?.();
        },
      }
    );

    return unsubscribe;
  }

  /**
   * Set authentication token
   */
  setAuthToken(token: string): void {
    this.config.apiKey = token;

    // Update HTTP client headers
    this.client.setHeaders(this.getDefaultHeaders());

    // Reconnect WebSocket client with new auth
    if (this.wsClient) {
      this.wsClient.terminate();
      this.initializeWSClient();
    }
  }

  /**
   * Clear authentication
   */
  clearAuth(): void {
    delete this.config.apiKey;
    this.client.setHeaders(this.getDefaultHeaders());

    if (this.wsClient) {
      this.wsClient.terminate();
      this.initializeWSClient();
    }
  }

  /**
   * Check if client is authenticated
   */
  isAuthenticated(): boolean {
    return !!this.config.apiKey;
  }

  /**
   * Close all connections
   */
  close(): void {
    if (this.wsClient) {
      this.wsClient.terminate();
    }
  }

  /**
   * Get default headers for requests
   */
  private getDefaultHeaders(): Record<string, string> {
    const headers: Record<string, string> = {
      'Content-Type': 'application/json',
      'User-Agent': `wrfcoin-js-sdk/1.0.0`,
    };

    if (this.config.apiKey) {
      headers['Authorization'] = `Bearer ${this.config.apiKey}`;
    }

    return headers;
  }

  /**
   * Initialize WebSocket client
   */
  private initializeWSClient(): void {
    const connectionParams: Record<string, any> = {};

    if (this.config.apiKey) {
      connectionParams.Authorization = `Bearer ${this.config.apiKey}`;
    }

    this.wsClient = createClient({
      url: this.config.wsEndpoint!,
      connectionParams,
      retryAttempts: this.config.retries,
      shouldRetry: () => true,
    });
  }

  /**
   * Handle and normalize errors
   */
  private handleError(error: any): WrfcoinError {
    if (error instanceof WrfcoinError) {
      return error;
    }

    if (error.response) {
      const { status, errors } = error.response;

      if (status === 401 || status === 403) {
        return new AuthenticationError(
          errors?.[0]?.message || 'Authentication failed'
        );
      }

      if (status >= 500) {
        return new NetworkError(
          errors?.[0]?.message || 'Server error occurred'
        );
      }

      return new WrfcoinError(
        errors?.[0]?.message || error.message || 'GraphQL request failed'
      );
    }

    if (error.code === 'NETWORK_ERROR' || error.name === 'FetchError') {
      return new NetworkError('Network connection failed');
    }

    return new WrfcoinError(error.message || 'Unknown error occurred');
  }

  /**
   * Determine if error is retryable
   */
  private shouldRetry(error: WrfcoinError): boolean {
    return (
      error instanceof NetworkError ||
      error.message.includes('timeout') ||
      error.message.includes('ECONNRESET')
    );
  }

  /**
   * Delay utility for retries
   */
  private delay(ms: number): Promise<void> {
    return new Promise(resolve => setTimeout(resolve, ms));
  }
}
