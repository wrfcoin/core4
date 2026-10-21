import { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js';
import { z } from 'zod';
import type { ApiClient } from '../client.js';
import { formatApiError } from '../client.js';

/**
 * Read-only chain primitive tools that query the native-chain Rust node directly
 * (port 8556, configurable via WRFCOIN_CHAIN_HOST).
 *
 * These expose block lookup, address balance, and validator metrics without
 * requiring callers to know the underlying REST API paths.
 */
export function registerChainTools(server: McpServer, client: ApiClient): void {

  server.registerTool(
    'get_block_by_height',
    {
      title: 'Get Block by Height',
      description: 'Look up a specific block on the WRFCoin chain by its height (block number). Returns the full block including hash, timestamp, transaction count, and proposer.',
      inputSchema: z.object({
        height: z.number().int().min(0).describe('Block height (0-indexed)'),
      }),
    },
    async ({ height }) => {
      try {
        const response = await client.chain.get(`/chain/block/${height}`);
        return { content: [{ type: 'text' as const, text: JSON.stringify(response.data, null, 2) }] };
      } catch (error) {
        return { content: [{ type: 'text' as const, text: formatApiError(error) }], isError: true };
      }
    }
  );

  server.registerTool(
    'get_address_balance',
    {
      title: 'Get Address Balance',
      description: 'Look up the WRF token balance and staking details for a chain address. Returns balance, stake_required, stake_locked, spendable_balance, and earned_balance_total.',
      inputSchema: z.object({
        address: z.string().describe('20-byte hex address (with or without 0x prefix)'),
      }),
    },
    async ({ address }) => {
      try {
        const response = await client.chain.get(`/addresses/${encodeURIComponent(address)}`);
        return { content: [{ type: 'text' as const, text: JSON.stringify(response.data, null, 2) }] };
      } catch (error) {
        return { content: [{ type: 'text' as const, text: formatApiError(error) }], isError: true };
      }
    }
  );

  server.registerTool(
    'get_validator_metrics',
    {
      title: 'Get Validator Metrics',
      description: 'Get network-wide validator stake metrics: total account count, aggregate stake required and locked, total spendable and earned balances, and network stake maturity percentage.',
      inputSchema: z.object({}),
    },
    async () => {
      try {
        const response = await client.chain.get('/transactions/stake/metrics');
        return { content: [{ type: 'text' as const, text: JSON.stringify(response.data, null, 2) }] };
      } catch (error) {
        return { content: [{ type: 'text' as const, text: formatApiError(error) }], isError: true };
      }
    }
  );

  server.registerTool('get_address', {
    title: 'Get Address', description: 'Read the node account JSON; may require a Reader bearer token.',
    inputSchema: z.object({ address: z.string().min(1).max(256) }),
  }, async ({ address }) => {
    try {
      const response = await client.chain.get(`/addresses/${encodeURIComponent(address)}`);
      return { content: [{ type: 'text' as const, text: JSON.stringify(response.data, null, 2) }] };
    } catch (error) { return { content: [{ type: 'text' as const, text: formatApiError(error) }], isError: true }; }
  });

  server.registerTool('get_commit_status', {
    title: 'Get Commit Status', description: 'Read contribution commit status from the node; may require a Reader bearer token.',
    inputSchema: z.object({ commitId: z.string().min(1).max(256) }),
  }, async ({ commitId }) => {
    try {
      const response = await client.chain.get(`/transactions/commit/${encodeURIComponent(commitId)}/status`);
      return { content: [{ type: 'text' as const, text: JSON.stringify(response.data, null, 2) }] };
    } catch (error) { return { content: [{ type: 'text' as const, text: formatApiError(error) }], isError: true }; }
  });

  server.registerTool('get_oracle_status', {
    title: 'Get Oracle Status', description: 'Read oracle status from the node; may require a Reader bearer token.',
    inputSchema: z.object({}),
  }, async () => {
    try {
      const response = await client.chain.get('/transactions/oracle/status');
      return { content: [{ type: 'text' as const, text: JSON.stringify(response.data, null, 2) }] };
    } catch (error) { return { content: [{ type: 'text' as const, text: formatApiError(error) }], isError: true }; }
  });

  server.registerTool('get_readyz', {
    title: 'Get Node Readiness', description: 'Read the node readiness JSON without inventing availability.',
    inputSchema: z.object({}),
  }, async () => {
    try {
      const response = await client.chain.get('/readyz');
      return { content: [{ type: 'text' as const, text: JSON.stringify(response.data, null, 2) }] };
    } catch (error) { return { content: [{ type: 'text' as const, text: formatApiError(error) }], isError: true }; }
  });
}
