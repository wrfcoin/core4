import { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js';
import type { ApiClient } from '../client.js';
import { formatApiError } from '../client.js';

export function registerChainStatusResource(server: McpServer, client: ApiClient): void {
  server.registerResource(
    'chain-status',
    'wrfcoin://chain/status',
    {
      title: 'WRFCoin Blockchain Status',
      description: 'Live blockchain status including block height, consensus state, peer count, and sync status',
      mimeType: 'application/json',
    },
    async (uri) => {
      try {
        const response = await client.api.get('/api/blockchain/status');
        return {
          contents: [{
            uri: uri.href,
            mimeType: 'application/json',
            text: JSON.stringify(response.data, null, 2),
          }],
        };
      } catch (error) {
        return {
          contents: [{
            uri: uri.href,
            mimeType: 'text/plain',
            text: `Error fetching chain status: ${formatApiError(error)}`,
          }],
        };
      }
    }
  );
}
