import { config as dotenvConfig } from 'dotenv';
import { resolve, dirname } from 'path';
import { fileURLToPath } from 'url';

const __dirname = dirname(fileURLToPath(import.meta.url));
dotenvConfig({ path: resolve(__dirname, '..', '.env') });

export interface ServiceEndpoints {
  api: string;        // Unified backend entrypoint (source default port 3001)
  wallet: string;     // Unified backend wallet routes, or explicit override
  weather: string;    // Unified backend weather routes, or explicit override
  governance: string; // Unified backend governance routes, or explicit override
  chain: string;      // Port 8556 — Native-chain Rust node (read-only primitives)
}

export interface Config {
  apiKey: string | undefined;
  jwtToken: string | undefined;
  services: ServiceEndpoints;
  requestTimeout: number;
}

export function loadConfig(): Config {
  const backend = process.env.WRFCOIN_API_HOST ?? 'http://localhost:3001';
  return {
    apiKey: process.env.WRFCOIN_API_KEY,
    jwtToken: process.env.WRFCOIN_JWT_TOKEN,
    services: {
      api: backend,
      wallet: process.env.WRFCOIN_WALLET_HOST ?? backend,
      weather: process.env.WRFCOIN_WEATHER_HOST ?? backend,
      governance: process.env.WRFCOIN_GOVERNANCE_HOST ?? backend,
      chain: process.env.WRFCOIN_CHAIN_HOST ?? 'http://localhost:8556',
    },
    requestTimeout: parseInt(process.env.WRFCOIN_REQUEST_TIMEOUT ?? '10000', 10),
  };
}
