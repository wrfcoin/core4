/**
 * Jest stub for frontend/src/services/config.ts
 * Replaces Vite-only (import.meta.env / window) code so tests run under Node.
 */
'use strict';

const API_BASE_URL = 'http://localhost:4001/api';

module.exports = {
  API_BASE_URL,
  HSM_API_URL: `${API_BASE_URL}/hsm`,
  WS_URL: 'ws://localhost:4001/ws',
  CROSS_CHAIN_API_URL: `${API_BASE_URL}/cross-chain`,
  WALLET_API_URL: `${API_BASE_URL}/wallet`,
  DEFAULT_PAGE_SIZE: 10,
  API_TIMEOUT: 10000,
  defaultConfig: {
    baseUrl: API_BASE_URL,
    timeout: 10000,
    retries: 3,
    wsUrl: 'ws://localhost:4001/ws',
  },
  environments: {
    development: { baseUrl: API_BASE_URL },
    production: { baseUrl: '/api' },
    test: { baseUrl: API_BASE_URL },
  },
  getCurrentConfig: () => ({
    baseUrl: API_BASE_URL,
    timeout: 10000,
    retries: 3,
    wsUrl: 'ws://localhost:4001/ws',
  }),
  isDevelopment: () => false,
  isProduction: () => false,
};
