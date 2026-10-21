import { notWired } from '../not-wired.js';
import { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js';
import { z } from 'zod';
import type { ApiClient } from '../client.js';
import { formatApiError } from '../client.js';

export function registerBlockchainTools(server: McpServer, client: ApiClient): void {

  server.registerTool(
    'get_chain_status',
    {
      title: 'Get Chain Status',
      description: 'Get current WRFCoin blockchain status: block height, network hash rate, consensus state, peer count, sync status, and uptime.',
      inputSchema: z.object({}),
    },
    async () => {
      try {
        const response = await client.api.get('/api/blockchain/status');
        return { content: [{ type: 'text' as const, text: JSON.stringify(response.data, null, 2) }] };
      } catch (error) {
        return { content: [{ type: 'text' as const, text: formatApiError(error) }], isError: true };
      }
    }
  );

  server.registerTool(
    'list_blocks',
    {
      title: 'List Blocks',
      description: 'Retrieve a paginated list of blockchain blocks. Returns block hash, index, timestamp, transaction count, and size.',
      inputSchema: z.object({
        limit: z.number().min(1).max(100).optional().describe('Number of blocks to return (default 10, max 100)'),
        offset: z.number().min(0).optional().describe('Number of blocks to skip'),
        order: z.enum(['asc', 'desc']).optional().describe('Not wired: explicit sort ordering is unavailable'),
      }),
    },
    async ({ limit, offset, order }) => {
      if (order !== undefined) return notWired('list_blocks', 'backend does not support the requested order parameter');
      try {
        const params: Record<string, string | number> = {};
        if (limit !== undefined) params.limit = limit;
        if (offset !== undefined) params.offset = offset;
        const response = await client.api.get('/api/blockchain/blocks', { params });
        return { content: [{ type: 'text' as const, text: JSON.stringify(response.data, null, 2) }] };
      } catch (error) {
        return { content: [{ type: 'text' as const, text: formatApiError(error) }], isError: true };
      }
    }
  );

  server.registerTool(
    'get_block',
    {
      title: 'Get Block',
      description: 'Not wired: backend block lookup accepts height, not hash',
      inputSchema: z.object({
        blockHash: z.string().describe('The block hash to look up'),
      }),
    },
    async () => notWired('get_block', 'backend block lookup accepts height, not hash')
  );

  server.registerTool(
    'get_transaction',
    {
      title: 'Get Transaction',
      description: 'Not wired: backend transaction-by-hash endpoint returns unavailable',
      inputSchema: z.object({
        txHash: z.string().describe('The transaction hash to look up'),
      }),
    },
    async () => notWired('get_transaction', 'backend transaction-by-hash endpoint returns unavailable')
  );
}
