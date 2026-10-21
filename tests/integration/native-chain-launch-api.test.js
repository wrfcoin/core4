const axios = require('axios');
const crypto = require('crypto');
const { ed25519 } = require('@noble/curves/ed25519');
const { bytesToHex, utf8ToBytes } = require('@noble/hashes/utils.js');

const API_BASE = process.env.WRFCOIN_API_BASE_URL || 'http://127.0.0.1:8556';
const REQUEST_TIMEOUT_MS = 15000;
const READER_TOKEN = process.env.WRFCOIN_READER_TOKEN || '';
const readerHeaders = () => (READER_TOKEN ? { Authorization: `Bearer ${READER_TOKEN}` } : {});

const canonicalizeJson = value => {
  if (Array.isArray(value)) {
    return value.map(canonicalizeJson);
  }
  if (value && typeof value === 'object') {
    return Object.keys(value)
      .sort()
      .reduce((acc, key) => {
        acc[key] = canonicalizeJson(value[key]);
        return acc;
      }, {});
  }
  return value;
};

const makeCommitId = ({ payload, nonce, stationId, timestamp, contributionType }) => {
  const canonicalPayload = canonicalizeJson(payload);
  const preimage = `${JSON.stringify(canonicalPayload)}||${nonce}||${stationId}||${timestamp}||${contributionType}`;
  return crypto.createHash('sha3-256').update(preimage).digest('hex');
};

const createOracleIdentity = () => {
  const privateKey = ed25519.utils.randomSecretKey();
  const publicKey = bytesToHex(ed25519.getPublicKey(privateKey));
  return { privateKey, publicKey };
};

const signOracleAttestation = ({ privateKey, commitHash, oracleId, approved, timestamp }) => {
  const message = `${commitHash}|${oracleId}|${approved}|${timestamp}`;
  const signatureBytes = ed25519.sign(utf8ToBytes(message), privateKey);
  return bytesToHex(signatureBytes);
};

const oraclePrimary = createOracleIdentity();
const oracleBackup = createOracleIdentity();

describe('Native Chain Launch API Flow', () => {
  beforeAll(async () => {
    await axios.get(`${API_BASE}/health`, { timeout: REQUEST_TIMEOUT_MS });
  });

  test('stake-status endpoint returns schema for valid address input', async () => {
    const address = '0x0000000000000000000000000000000000000001';
    const response = await axios.get(`${API_BASE}/transactions/stake/${address}`, {
      timeout: REQUEST_TIMEOUT_MS,
      headers: readerHeaders(),
    });

    expect(response.status).toBe(200);
    expect(response.data).toMatchObject({
      address,
    });
    expect(response.data).toHaveProperty('stake_required');
    expect(response.data).toHaveProperty('stake_locked');
    expect(response.data).toHaveProperty('spendable_balance');
    expect(response.data).toHaveProperty('earned_balance_total');
    expect(response.data).toHaveProperty('maturity_percent');
  });

  test('oracle-config endpoint updates and returns quorum settings', async () => {
    const updateResponse = await axios.put(
      `${API_BASE}/transactions/oracle/config`,
      {
        oracle_set: [oraclePrimary.publicKey, oracleBackup.publicKey],
        oracle_quorum: 1,
      },
      { timeout: REQUEST_TIMEOUT_MS }
    );

    expect(updateResponse.status).toBe(200);
    expect(updateResponse.data).toMatchObject({
      oracle_quorum: 1,
    });
    expect(updateResponse.data.oracle_set).toEqual(
      expect.arrayContaining([oraclePrimary.publicKey, oracleBackup.publicKey])
    );

    const getResponse = await axios.get(`${API_BASE}/transactions/oracle/config`, {
      timeout: REQUEST_TIMEOUT_MS,
      headers: readerHeaders(),
    });
    expect(getResponse.status).toBe(200);
    expect(getResponse.data.oracle_quorum).toBe(1);
    expect(getResponse.data.oracle_set).toEqual(
      expect.arrayContaining([oraclePrimary.publicKey])
    );

    const statusResponse = await axios.get(`${API_BASE}/transactions/oracle/status`, {
      timeout: REQUEST_TIMEOUT_MS,
      headers: readerHeaders(),
    });
    expect(statusResponse.status).toBe(200);
    expect(statusResponse.data).toMatchObject({
      oracle_quorum: 1,
    });
    expect(statusResponse.data).toHaveProperty('tracked_commit_count');
    expect(statusResponse.data).toHaveProperty('commits_with_quorum');
    expect(statusResponse.data).toHaveProperty('commits_pending_quorum');
  });

  test('commit/reveal/attestation submissions are accepted and visible in contribution status view', async () => {
    const contributor = '0x0000000000000000000000000000000000000002';
    const miner = '0x0000000000000000000000000000000000000003';
    const oracleId = oraclePrimary.publicKey;
    const stationId = 'station-launch-e2e';
    const payload = { temperature: 24.6, humidity: 0.53, wind_mps: 3.4 };
    const nonce = `nonce-${Date.now()}`;
    const timestamp = Math.floor(Date.now() / 1000);

    const commitHash = makeCommitId({
      payload,
      nonce,
      stationId,
      timestamp,
      contributionType: 'contribution',
    });

    await axios.put(
      `${API_BASE}/transactions/oracle/config`,
      {
        oracle_set: [oracleId, oracleBackup.publicKey],
        oracle_quorum: 1,
      },
      { timeout: REQUEST_TIMEOUT_MS }
    );

    const commitResponse = await axios.post(
      `${API_BASE}/transactions/contribution/commit`,
      {
        contributor,
        station_id: stationId,
        commit_hash: commitHash,
      },
      { timeout: REQUEST_TIMEOUT_MS }
    );

    expect(commitResponse.status).toBe(201);
    expect(commitResponse.data).toHaveProperty('tx_type', 'ContributionCommit');

    const revealResponse = await axios.post(
      `${API_BASE}/transactions/contribution/reveal`,
      {
        contributor,
        station_id: stationId,
        payload,
        nonce,
        commit_hash: commitHash,
        timestamp,
      },
      { timeout: REQUEST_TIMEOUT_MS }
    );

    expect(revealResponse.status).toBe(201);
    expect(revealResponse.data).toHaveProperty('tx_type', 'ContributionReveal');

    const attestationResponse = await axios.post(
      `${API_BASE}/transactions/oracle/attestation`,
      {
        oracle_id: oracleId,
        commit_hash: commitHash,
        approved: true,
        signature: signOracleAttestation({
          privateKey: oraclePrimary.privateKey,
          commitHash,
          oracleId,
          approved: true,
          timestamp,
        }),
        confidence: 0.9,
        timestamp,
      },
      { timeout: REQUEST_TIMEOUT_MS }
    );

    expect(attestationResponse.status).toBe(201);
    expect(attestationResponse.data).toHaveProperty('tx_type', 'OracleAttestation');

    const mineResponse = await axios.post(
      `${API_BASE}/blocks/mine`,
      {
        miner_address: miner,
      },
      { timeout: REQUEST_TIMEOUT_MS }
    );

    expect(mineResponse.status).toBe(201);

    const statusResponse = await axios.get(`${API_BASE}/transactions/contributions/status`, {
      timeout: REQUEST_TIMEOUT_MS,
      headers: readerHeaders(),
    });

    expect(statusResponse.status).toBe(200);
    expect(Array.isArray(statusResponse.data.items)).toBe(true);

    const commitStatus = statusResponse.data.items.find(item => item.commit_id === commitHash);
    expect(commitStatus).toBeDefined();
    expect(commitStatus.status).toMatch(/revealed|committed|attested/);

    const directStatus = await axios.get(
      `${API_BASE}/transactions/commit/${commitHash}/status`,
      { timeout: REQUEST_TIMEOUT_MS }
    );

    expect(directStatus.status).toBe(200);
    expect(directStatus.data).toHaveProperty('commit_id', commitHash);

    const stakeMetrics = await axios.get(`${API_BASE}/transactions/stake/metrics`, {
      timeout: REQUEST_TIMEOUT_MS,
    });
    expect(stakeMetrics.status).toBe(200);
    expect(stakeMetrics.data).toHaveProperty('account_count');
    expect(stakeMetrics.data).toHaveProperty('total_stake_locked');
    expect(stakeMetrics.data).toHaveProperty('network_stake_maturity_percent');
  });

  test('signing-key endpoints require authentication (regression test for core4#1258)', async () => {
    // Test /transactions/signing-keys/{role} without auth
    try {
      await axios.get(`${API_BASE}/transactions/signing-keys/validator`, {
        timeout: REQUEST_TIMEOUT_MS,
      });
      fail('Expected 401 UNAUTHORIZED but request succeeded');
    } catch (error) {
      expect(error.response.status).toBe(401);
      expect(error.response.data).toMatchObject({
        error_type: 'UNAUTHORIZED',
      });
    }

    // Test /network/signing-keys/{role} without auth
    try {
      await axios.get(`${API_BASE}/network/signing-keys/oracle`, {
        timeout: REQUEST_TIMEOUT_MS,
      });
      fail('Expected 401 UNAUTHORIZED but request succeeded');
    } catch (error) {
      expect(error.response.status).toBe(401);
      expect(error.response.data).toMatchObject({
        error_type: 'UNAUTHORIZED',
      });
    }
  });

  test('network audit log requires authentication (regression test for core4#1261)', async () => {
    // Test /network/audit-log without auth
    try {
      await axios.get(`${API_BASE}/network/audit-log`, {
        timeout: REQUEST_TIMEOUT_MS,
      });
      fail('Expected 401 UNAUTHORIZED but request succeeded');
    } catch (error) {
      expect(error.response.status).toBe(401);
      expect(error.response.data).toMatchObject({
        error_type: 'UNAUTHORIZED',
      });
    }

    // Test /network/audit-log with query parameters but no auth
    try {
      await axios.get(`${API_BASE}/network/audit-log?kind=governance_action&offset=0&limit=10`, {
        timeout: REQUEST_TIMEOUT_MS,
      });
      fail('Expected 401 UNAUTHORIZED but request succeeded');
    } catch (error) {
      expect(error.response.status).toBe(401);
      expect(error.response.data).toMatchObject({
        error_type: 'UNAUTHORIZED',
      });
    }
  });
});
