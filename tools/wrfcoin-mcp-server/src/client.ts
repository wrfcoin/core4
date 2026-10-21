import axios, { AxiosInstance, AxiosError } from 'axios';
import type { Config } from './config.js';

export interface ApiClient {
  api: AxiosInstance;
  wallet: AxiosInstance;
  weather: AxiosInstance;
  governance: AxiosInstance;
  chain: AxiosInstance;
}

export function createApiClient(config: Config): ApiClient {
  const createInstance = (baseURL: string): AxiosInstance => {
    const headers: Record<string, string> = {
      'Content-Type': 'application/json',
    };
    if (config.apiKey) {
      headers['X-API-Key'] = config.apiKey;
    }
    if (config.jwtToken) {
      headers['Authorization'] = `Bearer ${config.jwtToken}`;
    }

    return axios.create({
      baseURL,
      timeout: config.requestTimeout,
      headers,
    });
  };

  return {
    api: createInstance(config.services.api),
    wallet: createInstance(config.services.wallet),
    weather: createInstance(config.services.weather),
    governance: createInstance(config.services.governance),
    chain: createInstance(config.services.chain),
  };
}

export function formatApiError(error: unknown): string {
  if (error instanceof AxiosError) {
    if (error.code === 'ECONNREFUSED') {
      return `Service unavailable: could not connect to ${error.config?.baseURL}. Is the service running?`;
    }
    if (error.response) {
      const data = error.response.data;
      const msg = typeof data === 'object' && data?.error
        ? data.error
        : JSON.stringify(data);
      return `API error (${error.response.status}): ${msg}`;
    }
    if (error.code === 'ECONNABORTED') {
      return `Request timed out after ${error.config?.timeout}ms`;
    }
    return `Network error: ${error.message}`;
  }
  if (error instanceof Error) {
    return error.message;
  }
  return String(error);
}
