import { notWired } from '../not-wired.js';
import { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js';
import { z } from 'zod';
import type { ApiClient } from '../client.js';
import { formatApiError } from '../client.js';

export function registerWalletTools(server: McpServer, client: ApiClient): void {

  // 1. generate_mnemonic - POST /api/wallet/mnemonic/generate
  server.registerTool(
    'generate_mnemonic',
    {
      title: 'Generate Mnemonic',
      description: 'Generate a new BIP-39 mnemonic seed phrase for wallet creation. Returns a 12 or 24 word phrase.',
      inputSchema: z.object({}),
    },
    async () => {
      try {
        const response = await client.wallet.post('/api/wallet/mnemonic/generate');
        return { content: [{ type: 'text' as const, text: JSON.stringify(response.data, null, 2) }] };
      } catch (error) {
        return { content: [{ type: 'text' as const, text: formatApiError(error) }], isError: true };
      }
    }
  );

  // 2. validate_mnemonic - POST /api/wallet/mnemonic/validate
  server.registerTool(
    'validate_mnemonic',
    {
      title: 'Validate Mnemonic',
      description: 'Validate a BIP-39 mnemonic seed phrase for correctness (checksum, word list).',
      inputSchema: z.object({
        mnemonic: z.string().describe('The mnemonic phrase to validate (space-separated words)'),
      }),
    },
    async ({ mnemonic }) => {
      try {
        const response = await client.wallet.post('/api/wallet/mnemonic/validate', { mnemonic });
        return { content: [{ type: 'text' as const, text: JSON.stringify(response.data, null, 2) }] };
      } catch (error) {
        return { content: [{ type: 'text' as const, text: formatApiError(error) }], isError: true };
      }
    }
  );

  // 3. list_hardware_wallets - GET /api/wallet/hardware/devices
  server.registerTool(
    'list_hardware_wallets',
    {
      title: 'List Hardware Wallets',
      description: 'Not wired: backend has no hardware discovery endpoint',
      inputSchema: z.object({}),
    },
    async () => notWired('list_hardware_wallets', 'backend has no hardware discovery endpoint')
  );
}
