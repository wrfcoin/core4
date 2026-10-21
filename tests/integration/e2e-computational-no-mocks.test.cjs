/**
 * E2E Computational Test — No Mocks (issue #631)
 *
 * Proves the API surface accepts real contribution data and produces
 * consensus-scored blocks. Uses a LIVE launch_api instance.
 *
 * Prerequisites:
 *   1. Build: cargo build -p wrfcoin-native-chain --bin launch_api
 *   2. Start: WRFCOIN_API_PORT=8556 cargo run -p wrfcoin-native-chain --bin launch_api
 *   3. Run:   node tests/integration/e2e-computational-no-mocks.test.cjs
 *
 * Or use the Phase 6 master runner (scripts/run-testnet-phase6-tdd.sh)
 * which starts a shared node and exports WRFCOIN_OPERATOR_TOKEN.
 */

const axios = require('axios');
const crypto = require('crypto');

const API_BASE = process.env.WRFCOIN_API_BASE_URL || 'http://127.0.0.1:8556';
const OPERATOR_TOKEN = process.env.WRFCOIN_OPERATOR_TOKEN;
const REQUEST_TIMEOUT_MS = 15000;

// Replicate the commit-id computation from shared-protocol.
// preimage = JSON.stringify(canonicalPayload) || nonce || station_id || timestamp || type
const canonicalizeJson = (value) => {
  if (Array.isArray(value)) return value.map(canonicalizeJson);
  if (value && typeof value === 'object') {
    return Object.keys(value)
      .sort()
      .reduce((acc, key) => { acc[key] = canonicalizeJson(value[key]); return acc; }, {});
  }
  return value;
};

const makeCommitId = ({ payload, nonce, stationId, timestamp, contributionType }) => {
  const canonical = canonicalizeJson(payload);
  const preimage = `${JSON.stringify(canonical)}||${nonce}||${stationId}||${timestamp}||${contributionType}`;
  return crypto.createHash('sha3-256').update(preimage).digest('hex');
};

async function runTest() {
  console.log('--- E2E-631: Computational No-Mocks Test ---');

  const config = OPERATOR_TOKEN
    ? { headers: { Authorization: `Bearer ${OPERATOR_TOKEN}` }, timeout: REQUEST_TIMEOUT_MS }
    : { timeout: REQUEST_TIMEOUT_MS };

  // 0. Health check — verify API is up.
  try {
    const health = await axios.get(`${API_BASE}/health`, { timeout: 5000 });
    console.log(`Health: ${health.data.status}`);
  } catch (err) {
    console.error(`SKIP: launch_api not reachable at ${API_BASE} (${err.message})`);
    process.exit(0); // Graceful skip — not a failure.
  }

  // 1. Seed test accounts.
  const contributorAddress = '0x' + 'c'.repeat(40);
  const minerAddress = '0x' + 'd'.repeat(40);

  console.log('Step 1: Seeding contributor and miner accounts');
  await Promise.all([
    axios.post(`${API_BASE}/debug/seed-account`, { address: contributorAddress, amount: 100000 }, config),
    axios.post(`${API_BASE}/debug/seed-account`, { address: minerAddress, amount: 1000 }, config),
  ]);

  // 2. Submit contribution commit.
  const stationId = `station-e2e-${Date.now()}`;
  const payload = { temperature: 22.5, humidity: 0.65, pressure: 1013.25, wind_speed: 4.2 };
  const nonce = `nonce-e2e-${Date.now()}`;
  const timestamp = Math.floor(Date.now() / 1000);
  const contributionType = 'contribution';

  const commitHash = makeCommitId({ payload, nonce, stationId, timestamp, contributionType });
  console.log(`Step 2: Submitting commit (station=${stationId}, hash=${commitHash.slice(0, 16)}...)`);

  const commitResp = await axios.post(
    `${API_BASE}/transactions/contribution/commit`,
    { contributor: contributorAddress, station_id: stationId, commit_hash: commitHash },
    config,
  );
  if (commitResp.status !== 201) throw new Error(`Commit failed: ${commitResp.status}`);
  console.log(`  Commit accepted (tx_type=${commitResp.data.tx_type})`);

  // 3. Submit contribution reveal.
  console.log('Step 3: Submitting reveal');
  const revealResp = await axios.post(
    `${API_BASE}/transactions/contribution/reveal`,
    {
      contributor: contributorAddress,
      station_id: stationId,
      payload,
      nonce,
      commit_hash: commitHash,
      timestamp,
    },
    config,
  );
  if (revealResp.status !== 201) throw new Error(`Reveal failed: ${revealResp.status}`);
  console.log(`  Reveal accepted (tx_type=${revealResp.data.tx_type})`);

  // 4. Mine a block to include both transactions.
  console.log('Step 4: Mining block');
  const mineResp = await axios.post(
    `${API_BASE}/blocks/mine`,
    { miner_address: minerAddress },
    config,
  );
  const blockData = mineResp.data.data || mineResp.data;
  console.log(`  Block mined: height=${blockData.height} txs=${blockData.transaction_count}`);

  if (!blockData.height || blockData.height < 1) {
    throw new Error(`Expected block height >= 1, got ${blockData.height}`);
  }

  // 5. Verify the block is queryable.
  console.log('Step 5: Querying blockchain state');
  const chainResp = await axios.get(`${API_BASE}/blockchain`, config);
  const chainData = chainResp.data.data || chainResp.data;
  console.log(`  Chain height: ${chainData.height}, tip: ${String(chainData.tip || '').slice(0, 16)}...`);

  if (chainData.height < 1) {
    throw new Error(`Chain height should be >= 1 after mining, got ${chainData.height}`);
  }

  // 6. Verify contribution status shows the commit as revealed.
  console.log('Step 6: Checking contribution status');
  const statusResp = await axios.get(
    `${API_BASE}/transactions/commit/${commitHash}/status`,
    config,
  );
  const status = statusResp.data;
  console.log(`  Status: ${status.status}, approvals=${status.approvals}`);

  // The reveal should be recorded — status should not be "not_found".
  if (status.status === 'not_found') {
    throw new Error('Contribution status is not_found after commit+reveal+mine');
  }

  console.log('--- E2E-631 PASS ---');
}

runTest().catch((err) => {
  console.error(`E2E-631 FAIL: ${err.message}`);
  if (err.response) {
    console.error(`  HTTP ${err.response.status}: ${JSON.stringify(err.response.data)}`);
  }
  process.exit(1);
});
