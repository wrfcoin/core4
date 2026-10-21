import { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js';
import type { ApiClient } from '../client.js';
import { formatApiError } from '../client.js';

export function registerServicesHealthResource(server: McpServer, client: ApiClient): void {
  server.registerResource(
    'services-health',
    'wrfcoin://services/health',
    {
      title: 'WRFCoin Services Health Dashboard',
      description: 'Health status of all platform services, database, and infrastructure',
      mimeType: 'application/json',
    },
    async (uri) => {
      try {
        const response = await client.api.get('/api/health');
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
            text: `Error fetching health status: ${formatApiError(error)}`,
          }],
        };
      }
    }
  );
}
