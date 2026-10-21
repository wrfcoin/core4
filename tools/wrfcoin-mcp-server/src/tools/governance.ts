import { notWired } from '../not-wired.js';
import { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js';
import { z } from 'zod';
import type { ApiClient } from '../client.js';
import { formatApiError } from '../client.js';

export function registerGovernanceTools(server: McpServer, client: ApiClient): void {

  server.registerTool(
    'list_proposals',
    {
      title: 'List Governance Proposals',
      description: 'List backend governance proposals with a bounded limit. Explicit status filtering is not wired.',
      inputSchema: z.object({
        status: z.enum(['active', 'pending', 'executed', 'defeated']).optional().describe('Not wired: explicit status filtering is unavailable'),
        limit: z.number().min(1).max(50).optional().describe('Max number of proposals to return'),
      }),
    },
    async ({ status, limit }) => {
      if (status !== undefined) return notWired('list_proposals', 'backend does not support the requested status filter');
      try {
        const params: Record<string, string | number> = {};
        if (limit !== undefined) params.limit = limit;
        const response = await client.governance.get('/api/governance/proposals', { params });
        return { content: [{ type: 'text' as const, text: JSON.stringify(response.data, null, 2) }] };
      } catch (error) {
        return { content: [{ type: 'text' as const, text: formatApiError(error) }], isError: true };
      }
    }
  );

  server.registerTool(
    'create_proposal',
    {
      title: 'Create Governance Proposal',
      description: 'Not wired: backend requires an admin native-governance payload that this tool cannot construct',
      inputSchema: z.object({
        title: z.string().min(1).max(200).describe('Proposal title'),
        description: z.string().min(1).describe('Detailed proposal description'),
        actions: z.array(z.record(z.unknown())).describe('Actions to execute if the proposal passes'),
        votingPeriod: z.number().min(3600).describe('Voting period in seconds (minimum 1 hour)'),
      }),
    },
    async () => notWired('create_proposal', 'backend requires an admin native-governance payload that this tool cannot construct')
  );

  server.registerTool(
    'vote_on_proposal',
    {
      title: 'Vote on Proposal',
      description: 'Not wired: backend requires authenticated native-governance vote proof that this tool cannot construct',
      inputSchema: z.object({
        proposalId: z.string().describe('The proposal ID to vote on'),
        vote: z.enum(['for', 'against', 'abstain']).describe('Vote direction'),
        votingPower: z.string().optional().describe('Voting power to use (token amount as string)'),
        reason: z.string().optional().describe('Optional reason for the vote'),
      }),
    },
    async () => notWired('vote_on_proposal', 'backend requires authenticated native-governance vote proof that this tool cannot construct')
  );
}
