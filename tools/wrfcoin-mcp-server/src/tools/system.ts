import { notWired } from '../not-wired.js';
import { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js';
import { z } from 'zod';
import type { ApiClient } from '../client.js';
import { formatApiError } from '../client.js';

export function registerSystemTools(server: McpServer, client: ApiClient): void {

  server.registerTool(
    'health_check',
    {
      title: 'Health Check',
      description: 'Get comprehensive platform health status including service availability, database, blockchain node, and performance metrics.',
      inputSchema: z.object({}),
    },
    async () => {
      try {
        const response = await client.api.get('/api/health');
        return { content: [{ type: 'text' as const, text: JSON.stringify(response.data, null, 2) }] };
      } catch (error) {
        return { content: [{ type: 'text' as const, text: formatApiError(error) }], isError: true };
      }
    }
  );

  server.registerTool(
    'get_mining_rewards',
    {
      title: 'Get Mining Rewards',
      description: 'Not wired: backend has no mining reward-history endpoint',
      inputSchema: z.object({
        address: z.string().optional().describe('Wallet address to get rewards for'),
        type: z.enum(['weather_data', 'compute', 'validation', 'staking']).optional().describe('Type of mining reward'),
        limit: z.number().min(1).max(100).optional().describe('Number of reward entries to return'),
      }),
    },
    async () => notWired('get_mining_rewards', 'backend has no mining reward-history endpoint')
  );
}
