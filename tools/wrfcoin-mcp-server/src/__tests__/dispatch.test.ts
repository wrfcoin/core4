import assert from 'node:assert/strict';
import test from 'node:test';
import axios from 'axios';
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js';
import { InMemoryTransport } from '@modelcontextprotocol/sdk/inMemory.js';
import type { ApiClient } from '../client.js';
import { registerWalletTools } from '../tools/wallet.js';
import { registerWeatherTools } from '../tools/weather.js';
import { registerChainTools } from '../tools/chain.js';
import { registerBlockchainTools } from '../tools/blockchain.js';
import { registerGovernanceTools } from '../tools/governance.js';
import { registerSystemTools } from '../tools/system.js';
import { registerWeatherLatestResource } from '../resources/weather-latest.js';

async function fixture() {
  const requests: { service: string; method: string; path: string; body: unknown; params: unknown }[] = [];
  const payload = { available: true, data: { synthetic: ['one', 2, null] } };
  const client = Object.fromEntries(['api', 'wallet', 'weather', 'governance', 'chain'].map(service => [service, axios.create({
    adapter: async config => {
      requests.push({ service, method: config.method!, path: config.url!, body: config.data ? JSON.parse(config.data) : undefined, params: config.params });
      return { data: payload, status: 200, statusText: 'OK', headers: {}, config };
    },
  })])) as unknown as ApiClient;
  const server = new McpServer({ name: 'synthetic', version: '1' });
  for (const register of [registerWalletTools, registerWeatherTools, registerChainTools, registerBlockchainTools, registerGovernanceTools, registerSystemTools]) register(server, client);
  registerWeatherLatestResource(server, client);
  const peer = new Client({ name: 'synthetic-test', version: '1' });
  const [a, b] = InMemoryTransport.createLinkedPair();
  await Promise.all([peer.connect(a), server.connect(b)]);
  return { peer, requests, payload, dispose: async () => { await peer.close(); await server.close(); } };
}

const refused: [string, Record<string, unknown>][] = [
  ['list_hardware_wallets', {}],
  ['submit_weather_data', { stationId: 'synthetic', sensorData: [{ type: 'temperature', value: 20, unit: 'celsius' }], latitude: 0, longitude: 0 }],
  ['query_weather_data', {}],
  ['get_weather_security_status', {}],
  ['get_block', { blockHash: 'synthetic-hash' }],
  ['get_transaction', { txHash: 'synthetic-hash' }],
  ['get_mining_rewards', {}],
  ['create_proposal', { title: 'synthetic', description: 'synthetic', actions: [], votingPeriod: 3600 }],
  ['vote_on_proposal', { proposalId: 'synthetic', vote: 'for' }],
  ['list_blocks', { order: 'asc' }],
  ['list_proposals', { status: 'active' }],
];
for (const [name, args] of refused) test(`${name} refuses before HTTP through MCP dispatch`, async () => {
  const f = await fixture();
  try {
    const result = await f.peer.callTool({ name, arguments: args });
    assert.equal(result.isError, true);
    assert.match(JSON.stringify(result.content), /not wired.*#3249/);
    assert.deepEqual(f.requests, []);
  } finally { await f.dispose(); }
});

for (const [name, args, path, body] of [
  ['generate_mnemonic', {}, '/api/wallet/mnemonic/generate', undefined],
  ['validate_mnemonic', { mnemonic: 'synthetic invalid phrase' }, '/api/wallet/mnemonic/validate', { mnemonic: 'synthetic invalid phrase' }],
] as const) test(`${name} uses the mounted backend route and preserves JSON`, async () => {
  const f = await fixture();
  try {
    const result = await f.peer.callTool({ name, arguments: args });
    assert.deepEqual(JSON.parse((result.content as { text: string }[])[0].text), f.payload);
    assert.deepEqual(f.requests, [{ service: 'wallet', method: 'post', path, body, params: undefined }]);
  } finally { await f.dispose(); }
});

for (const [name, args, path] of [
  ['get_block_by_height', { height: 7 }, '/chain/block/7'],
  ['get_address_balance', { address: 'synthetic/?' }, '/addresses/synthetic%2F%3F'],
  ['get_validator_metrics', {}, '/transactions/stake/metrics'],
  ['get_address', { address: 'synthetic/?' }, '/addresses/synthetic%2F%3F'],
  ['get_commit_status', { commitId: 'synthetic/?' }, '/transactions/commit/synthetic%2F%3F/status'],
  ['get_oracle_status', {}, '/transactions/oracle/status'],
  ['get_readyz', {}, '/readyz'],
] as const) test(`${name} requests the node route and preserves JSON`, async () => {
  const f = await fixture();
  try {
    const result = await f.peer.callTool({ name, arguments: args });
    assert.deepEqual(JSON.parse((result.content as { text: string }[])[0].text), f.payload);
    assert.deepEqual(f.requests, [{ service: 'chain', method: 'get', path, body: undefined, params: undefined }]);
  } finally { await f.dispose(); }
});

for (const [name, args, service, path, params] of [
  ['get_chain_status', {}, 'api', '/api/blockchain/status', undefined],
  ['list_blocks', { limit: 5, offset: 2 }, 'api', '/api/blockchain/blocks', { limit: 5, offset: 2 }],
  ['list_proposals', { limit: 5 }, 'governance', '/api/governance/proposals', { limit: 5 }],
  ['health_check', {}, 'api', '/api/health', undefined],
] as const) test(`${name} preserves the verified backend query and JSON`, async () => {
  const f = await fixture();
  try {
    const result = await f.peer.callTool({ name, arguments: args });
    assert.deepEqual(JSON.parse((result.content as { text: string }[])[0].text), f.payload);
    assert.deepEqual(f.requests, [{ service, method: 'get', path, body: undefined, params }]);
  } finally { await f.dispose(); }
});

test('weather latest resource reports unavailable without HTTP', async () => {
  const f = await fixture();
  try {
    const result = await f.peer.readResource({ uri: 'wrfcoin://weather/latest' });
    assert.match(JSON.stringify(result.contents), /not wired.*#3249/);
    assert.deepEqual(f.requests, []);
  } finally { await f.dispose(); }
});
