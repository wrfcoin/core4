#!/usr/bin/env node
import { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js';
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js';

import { loadConfig } from './config.js';
import { createApiClient } from './client.js';

import { registerWeatherTools } from './tools/weather.js';
import { registerWalletTools } from './tools/wallet.js';
import { registerBlockchainTools } from './tools/blockchain.js';
import { registerGovernanceTools } from './tools/governance.js';
import { registerSystemTools } from './tools/system.js';
import { registerChainTools } from './tools/chain.js';

import { registerChainStatusResource } from './resources/chain-status.js';
import { registerServicesHealthResource } from './resources/services-health.js';
import { registerWeatherLatestResource } from './resources/weather-latest.js';

async function main(): Promise<void> {
  const config = loadConfig();
  const client = createApiClient(config);

  const server = new McpServer(
    {
      name: 'wrfcoin',
      version: '1.0.0',
    },
    {
      capabilities: {
        tools: {},
        resources: {},
      },
    }
  );

  // Register tools (22 total across 6 domains; unsupported tools refuse honestly)
  registerWeatherTools(server, client);
  registerWalletTools(server, client);
  registerBlockchainTools(server, client);
  registerGovernanceTools(server, client);
  registerSystemTools(server, client);
  registerChainTools(server, client);  // read-only chain primitives (#279)

  // Register resources (2 backend snapshots and an unavailable weather resource)
  registerChainStatusResource(server, client);
  registerServicesHealthResource(server, client);
  registerWeatherLatestResource(server, client);

  const transport = new StdioServerTransport();
  await server.connect(transport);

  console.error('WRFCoin MCP server running on stdio');
}

main().catch((error) => {
  console.error('Fatal error starting WRFCoin MCP server:', error);
  process.exit(1);
});
