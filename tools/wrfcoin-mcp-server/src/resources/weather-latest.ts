import { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js';
import type { ApiClient } from '../client.js';
import { notWired } from '../not-wired.js';

export function registerWeatherLatestResource(server: McpServer, _client: ApiClient): void {
  server.registerResource(
    'weather-latest',
    'wrfcoin://weather/latest',
    {
      title: 'Latest Weather Data',
      description: 'Not wired: backend has no unfiltered latest-weather endpoint',
      mimeType: 'application/json',
    },
    async (uri) => {
      return { contents: [{ uri: uri.href, mimeType: 'text/plain', text: notWired('weather-latest', 'backend has no unfiltered latest-weather endpoint').content[0].text }] };
    }
  );
}
