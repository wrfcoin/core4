/**
 * Mock Audit: Real-Mode Companion Tests (issue #631, Part C)
 *
 * Audit of tests/integration/ identified three files that mock the
 * execution-engine or blockchain adapter:
 *
 *   1. blockchain-api.test.js         — 100% mocked (jest.fn() for every API call)
 *   2. weather-to-blockchain.test.js  — 100% mocked (jest.fn() for weather + blockchain)
 *   3. native-chain-launch-api.test.js — ALREADY REAL-MODE (hits live launch_api)
 *
 * This file provides real-mode companion tests for the workflows covered
 * by blockchain-api.test.js and weather-to-blockchain.test.js, running
 * against a live launch_api instance. The existing mocked tests are
 * preserved untouched.
 *
 * Prerequisites:
 *   - launch_api running on port 8556 (or set WRFCOIN_API_BASE_URL)
 *   - WRFCOIN_OPERATOR_TOKEN set for auth (or auth disabled)
 *
 * Run:
 *   node tests/integration/e2e-mock-audit-companion.test.cjs
 */

const axios = require('axios');
const crypto = require('crypto');

const API_BASE = process.env.WRFCOIN_API_BASE_URL || 'http://127.0.0.1:8556';
const OPERATOR_TOKEN = process.env.WRFCOIN_OPERATOR_TOKEN;
const TIMEOUT = 15000;

const config = () => {
  const c = { timeout: TIMEOUT };
  if (OPERATOR_TOKEN) c.headers = { Authorization: `Bearer ${OPERATOR_TOKEN}` };
  return c;
};

const canonicalizeJson = (v) => {
  if (Array.isArray(v)) return v.map(canonicalizeJson);
  if (v && typeof v === 'object')
    return Object.keys(v).sort().reduce((a, k) => { a[k] = canonicalizeJson(v[k]); return a; }, {});
  return v;
};

const makeCommitId = ({ payload, nonce, stationId, timestamp, contributionType }) => {
  const pre = `${JSON.stringify(canonicalizeJson(payload))}||${nonce}||${stationId}||${timestamp}||${contributionType}`;
  return crypto.createHash('sha3-256').update(pre).digest('hex');
};

let passed = 0;
let failed = 0;

function assert(condition, msg) {
  if (!condition) { failed++; console.error(`  FAIL: ${msg}`); } else { passed++; }
}

async function testBlockchainApiCompanion() {
  console.log('\n[blockchain-api real-mode companion]');

  // Replaces: "should complete full wallet-to-transaction workflow" (mocked)
  console.log('  Test: seed account → mine block → query chain state');
  const addr = '0x' + crypto.randomBytes(20).toString('hex');
  const miner = '0x' + crypto.randomBytes(20).toString('hex');

  await axios.post(`${API_BASE}/debug/seed-account`, { address: addr, amount: 50000 }, config());
  await axios.post(`${API_BASE}/debug/seed-account`, { address: miner, amount: 1000 }, config());

  // Query balance
  const balResp = await axios.get(`${API_BASE}/addresses/${addr}`, config());
  assert(balResp.status === 200, `GET /addresses/${addr} should be 200`);
  assert(balResp.data.balance !== undefined, 'response should have balance field');

  // Mine a block
  const mineResp = await axios.post(`${API_BASE}/blocks/mine`, { miner_address: miner }, config());
  const block = mineResp.data.data || mineResp.data;
  assert(block.height >= 1, `block height should be >= 1, got ${block.height}`);
  assert(block.hash !== undefined, 'block should have a hash');

  // Replaces: "should query blockchain status" (mocked)
  console.log('  Test: query blockchain status');
  const chainResp = await axios.get(`${API_BASE}/blockchain`, config());
  const chain = chainResp.data.data || chainResp.data;
  assert(chain.height >= 1, `chain height should be >= 1, got ${chain.height}`);
}

async function testWeatherToBlockchainCompanion() {
  console.log('\n[weather-to-blockchain real-mode companion]');

  // Replaces: "should submit weather data and receive blockchain confirmation" (mocked)
  console.log('  Test: weather commit → reveal → mine → status check');
  const contributor = '0x' + crypto.randomBytes(20).toString('hex');
  const miner = '0x' + crypto.randomBytes(20).toString('hex');

  await axios.post(`${API_BASE}/debug/seed-account`, { address: contributor, amount: 10000 }, config());
  await axios.post(`${API_BASE}/debug/seed-account`, { address: miner, amount: 1000 }, config());

  const stationId = `wx-station-${Date.now()}`;
  const payload = { temperature: 18.3, humidity: 0.72, pressure: 1015.0 };
  const nonce = `wx-nonce-${Date.now()}`;
  const timestamp = Math.floor(Date.now() / 1000);

  const commitHash = makeCommitId({
    payload, nonce, stationId, timestamp, contributionType: 'contribution',
  });

  // Commit
  const commitResp = await axios.post(
    `${API_BASE}/transactions/contribution/commit`,
    { contributor, station_id: stationId, commit_hash: commitHash },
    config(),
  );
  assert(commitResp.status === 201, `commit should return 201, got ${commitResp.status}`);
  assert(commitResp.data.tx_type === 'ContributionCommit', 'tx_type should be ContributionCommit');

  // Reveal
  const revealResp = await axios.post(
    `${API_BASE}/transactions/contribution/reveal`,
    { contributor, station_id: stationId, payload, nonce, commit_hash: commitHash, timestamp },
    config(),
  );
  assert(revealResp.status === 201, `reveal should return 201, got ${revealResp.status}`);

  // Mine
  const mineResp = await axios.post(`${API_BASE}/blocks/mine`, { miner_address: miner }, config());
  const block = mineResp.data.data || mineResp.data;
  assert(block.height >= 1, `mined block height should be >= 1`);

  // Status
  const statusResp = await axios.get(
    `${API_BASE}/transactions/commit/${commitHash}/status`,
    config(),
  );
  assert(statusResp.status === 200, 'status endpoint should return 200');
  assert(statusResp.data.status !== 'not_found', `status should not be not_found, got ${statusResp.data.status}`);
}

async function run() {
  console.log('--- E2E-631 Part C: Mock Audit Real-Mode Companions ---');

  try {
    await axios.get(`${API_BASE}/health`, { timeout: 5000 });
  } catch (err) {
    console.log(`SKIP: launch_api not reachable at ${API_BASE} (${err.message})`);
    process.exit(0);
  }

  try { await testBlockchainApiCompanion(); } catch (e) {
    failed++; console.error(`  ERROR: ${e.message}`);
  }
  try { await testWeatherToBlockchainCompanion(); } catch (e) {
    failed++; console.error(`  ERROR: ${e.message}`);
  }

  console.log(`\n--- Results: ${passed} passed, ${failed} failed ---`);
  if (failed > 0) process.exit(1);
}

run();
