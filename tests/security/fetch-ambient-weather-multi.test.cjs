'use strict';

/**
 * Tests for the multi-contributor Ambient Weather collector and shared pipeline.
 *
 * These run without network access by:
 *   - exercising roster parsing/validation against the local fixture, and
 *   - driving the shared per-station fetch+submit loop with injected mock
 *     Ambient fetchers and a mock node httpPost, proving per-owner attribution.
 *
 * Run: node tests/security/fetch-ambient-weather-multi.test.cjs
 */

const assert = require('assert');
const fs = require('fs');
const path = require('path');

const pipeline = require('./lib/ambient-pipeline.cjs');
const multi = require('./fetch-ambient-weather-multi.cjs');
const single = require('./fetch-ambient-weather.cjs');
const TEST_MINER_ADDRESS = '0x680d51f7543ca70029866956376306d47ce60ed0';

function quorumPendingResponse(commitIds, overrides = {}) {
    return {
        status: 200,
        data: {
            chain_id: '777',
            network_id: 'testnet',
            height: overrides.height || 42,
            oracle_set: ['oracle-a', 'oracle-b', 'oracle-c'],
            oracle_quorum: 2,
            effective_quorum: 2,
            observations: [...commitIds].map((commitId) => ({
                commit_id: commitId,
                station_id: 'PWS-TEST',
                timestamp: 1786500000,
                nonce: 'nonce',
                contribution_type: 'contribution',
                data: {},
                source: 'mempool',
                revealed_at_height: null,
                provenance_class: 'verified_real',
                approvals: 0,
                rejections: 0,
                attesting_oracles: ['oracle-a', 'oracle-b'],
                has_quorum: false,
                reward_recorded: false,
                reward_finalized: false,
                ...overrides.observation,
            })),
            ready_to_mine: [],
            unrecoverable: [],
            ...overrides.response,
        },
    };
}

// #3266: awaiting fixtures model genuinely in-flight reveals, so the reading
// is stamped two minutes before now. A fixed historical date would put every
// fixture beyond the attester freshness window, where #3266 correctly
// terminalizes a reveal still short of quorum.
const IN_FLIGHT_READING_SECS = Math.floor(Date.now() / 1000) - 120;

function makeAwaitingEntry(commitId = 'commit-2774', stationId = 'PWS-2774') {
    return {
        commit_hash: commitId,
        nonce: '00112233445566778899aabbccddeeff', // pragma: allowlist secret — deterministic test fixture
        reading: {
            recorded_at: new Date(IN_FLIGHT_READING_SECS * 1000).toISOString(),
            temperature_c: 20,
        },
        commit_block_height: 100,
        station_id: stationId,
        contributor: '0x465230af3383c16d024c5f4b87ed2d1b595be148',
        timestamp: IN_FLIGHT_READING_SECS,
        quality_tier: 'good',
        phase: pipeline.PENDING_PHASE_AWAITING_QUORUM,
        reveal_submitted_at_height: 101,
    };
}

let passed = 0;
function test(name, fn) {
    return Promise.resolve()
        .then(fn)
        .then(() => { passed++; console.log(`  ok - ${name}`); })
        .catch((err) => { console.error(`  FAIL - ${name}: ${err.message}`); process.exitCode = 1; });
}

// --- Mock Ambient device + history for a station -----------------------------
function makeDevice(mac, name, model = 'WS-5000') {
    return {
        macAddress: mac,
        model,
        info: { name, model, coords: { lat: 1.0, lon: 2.0 }, location: name },
        lastData: { tempf: 50 },
    };
}
function makeHistory(count) {
    const out = [];
    const now = Date.now();
    for (let i = 0; i < count; i++) {
        out.push({
            dateutc: now - i * 60_000,
            date: new Date(now - i * 60_000).toISOString(),
            tempf: 50 + i,
            humidity: 60,
            baromrelin: 29.9,
            baromabsin: 29.9,
            windspeedmph: 5,
            windgustmph: 8,
            winddir: 180,
            hourlyrainin: 0,
            uv: 1,
            solarradiation: 100,
        });
    }
    return out;
}

async function run() {
    console.log('multi-tenant collector tests\n');

    // 1. roster fixture parses and filters correctly
    await test('loadRoster parses fixture, drops invalid + non-ambient entries', async () => {
        process.env.MULTI_TENANT_DISABLED = '';
        // Re-require with credentials-file via direct normalize calls (loadRoster reads CLI config
        // at import; here we validate the normalizer + fixture directly).
        const fixture = require('./fixtures/multi-tenant-credentials.json');
        const syntheticMacs = ['00:00:00:00:00:01', '00:00:00:00:00:02'];
        const normalized = fixture.map((e, i) => multi.normalizeRosterEntry(
            i < syntheticMacs.length ? { ...e, mac: syntheticMacs[i] } : e,
            i,
        )).filter(Boolean);
        assert.strictEqual(normalized.length, 2, 'expected 2 valid ambient stations');
        const ids = normalized.map((s) => s.stationId).sort();
        assert.deepStrictEqual(ids, ['synthetic-station-a', 'synthetic-station-b']);
    });

    // 2. normalizeRosterEntry requires the strict private roster contract
    await test('normalizeRosterEntry rejects malformed entries', () => {
        assert.strictEqual(multi.normalizeRosterEntry(null, 0), null);
        assert.strictEqual(multi.normalizeRosterEntry({ stationId: 'x' }, 0), null);
        assert.strictEqual(multi.normalizeRosterEntry({ provider: 'weatherflow', stationId: 'a', ownerAddress: 'b' }, 0), null);
        const ok = multi.normalizeRosterEntry({
            stationId: 'a', ownerAddress: 'b',
            mac: '00:00:00:00:00:03', model: 'WS-5000',
            consentStatus: 'approved', coarseMetadataStatus: 'approved',
            bindingRef: 'binding-b',
        }, 0);
        assert.ok(ok && ok.provider === 'ambient');
    });

    // 3. fetchStationReadings resolves by MAC and normalizes via injected mocks
    await test('fetchStationReadings uses injected fetchers, resolves by MAC', async () => {
        const station = {
            stationId: 'PWS-A', ownerAddress: '0xA', appKey: 'k', apiKey: 'k',
            mac: '00:00:00:00:00:03', model: 'WS-5000', latitude: 1, longitude: 2, elevation_m: 10,
        };
        const res = await pipeline.fetchStationReadings(station, {
            readingsLimit: 4,
            maxAgeMinutes: 100000, // ensure not stale
            sleepMs: 0,
            fetchDevices: async () => ({ status: 200, data: [makeDevice('00:00:00:00:00:03', 'A')] }),
            fetchDeviceHistory: async () => ({ status: 200, data: makeHistory(4) }),
        });
        assert.ok(res.ok, `fetch failed: ${res.detail}`);
        assert.strictEqual(res.normalized.length, 4);
        assert.strictEqual(res.stale, false);
        assert.ok(res.normalized[0].temperature_c != null);
    });

    await test('token-safe pressure trace reproduces normalization and validator arithmetic', () => {
        const fixture = require('./fixtures/pressure-rate-trace-2737.json');
        const previous = pipeline.normalizeReading(fixture.previous, fixture.station);
        const current = pipeline.normalizeReading(fixture.current, fixture.station);
        const elapsedSeconds = (
            new Date(current.recorded_at).getTime() - new Date(previous.recorded_at).getTime()
        ) / 1000;
        const delta = Math.abs(current.pressure_hpa - previous.pressure_hpa);
        const rate = delta / elapsedSeconds;

        assert.strictEqual(previous.pressure_hpa, fixture.expected.previous_pressure_hpa);
        assert.strictEqual(current.pressure_hpa, fixture.expected.current_pressure_hpa);
        assert.strictEqual(elapsedSeconds, fixture.expected.elapsed_seconds);
        assert.ok(Math.abs(delta - fixture.expected.absolute_delta_hpa) < 1e-9);
        assert.ok(Math.abs(rate - fixture.expected.rate_hpa_per_second) < 1e-9);
        assert.ok(rate > fixture.expected.validator_limit_hpa_per_second);
    });

    // 4. device_missing isolation when MAC not present
    await test('fetchStationReadings falls back to device lastData when history is empty', async () => {
        const station = {
            stationId: 'PWS-LATEST', ownerAddress: '0xLATEST', appKey: 'k', apiKey: 'k',
            mac: '00:00:00:00:00:05', model: 'WS-5000', latitude: 1, longitude: 2, elevation_m: 10,
        };
        const latest = makeHistory(1)[0];
        const device = makeDevice(station.mac, 'Latest');
        device.lastData = latest;
        const res = await pipeline.fetchStationReadings(station, {
            readingsLimit: 4,
            maxAgeMinutes: 100000,
            sleepMs: 0,
            fetchDevices: async () => ({ status: 200, data: [device] }),
            fetchDeviceHistory: async () => ({ status: 200, data: [] }),
        });
        assert.ok(res.ok, `fetch failed: ${res.detail}`);
        assert.strictEqual(res.normalized.length, 1);
        assert.strictEqual(res.rawCount, 1);
        assert.strictEqual(res.usedLatestFallback, true);
        assert.strictEqual(res.normalized[0].recorded_at, latest.dateutc);
    });

    // 5. device_missing isolation when MAC not present
    await test('fetchStationReadings returns device_missing when MAC absent', async () => {
        const station = { stationId: 'PWS-B', ownerAddress: '0xB', appKey: 'k', apiKey: 'k', mac: '00:00:00:00:00:04', model: 'WS-5000' };
        const res = await pipeline.fetchStationReadings(station, {
            sleepMs: 0,
            fetchDevices: async () => ({ status: 200, data: [makeDevice('00:00:00:00:00:03', 'A')] }),
            fetchDeviceHistory: async () => ({ status: 200, data: [] }),
        });
        assert.strictEqual(res.ok, false);
        assert.strictEqual(res.reason, 'device_missing');
    });

    // 5. submitStationReadings attributes commit/reveal under the given owner+station
    await test('submitStationReadings posts under correct contributor + stationId', async () => {
        const captured = [];
        const commitIds = new Set();
        let height = 10;
        const mockPost = async (host, port, urlPath, body) => {
            captured.push({ urlPath, contributor: body.contributor, station_id: body.station_id });
            if (urlPath.includes('/contribution/commit')) commitIds.add(body.commit_hash);
            if (urlPath === '/blocks/mine') height++;
            return { status: 201, data: { height } };
        };
        const mockGet = async () => quorumPendingResponse(commitIds, { height });
        const normalized = require('./lib/ambient-pipeline.cjs').normalizeReading
            ? makeHistory(2).map((r) => pipeline.normalizeReading(r, { elevation_m: 10, lat: 1, lon: 2 }))
            : [];

        const tmpState = path.join(require('os').tmpdir(), `pending-reveals-test-${process.pid}-${Date.now()}.json`);
        // Clear any leftover per-station watermark so dedup doesn't suppress this run.
        const wmPath = pipeline.getStationWatermarkPath('PWS-OWNER1', { stateFilePath: tmpState });
        try { require('fs').unlinkSync(wmPath); } catch (_) {}
        const result = await pipeline.submitStationReadings(normalized, 'PWS-OWNER1', '0xOWNER1', {
            host: 'h', port: 1, token: 't',
            httpPost: mockPost,
            httpGet: mockGet,
            stateFilePath: tmpState,
            commitBlockHeight: 10,
            nwsObsForTier: null,
            minerAddress: TEST_MINER_ADDRESS,
        });

        assert.strictEqual(result.committed, 2, 'both readings committed');
        assert.strictEqual(result.failed, 0);
        assert.ok(result.mined, 'quorum-qualified reveal block mined');
        // Every commit + reveal must carry the owner identity, none hardcoded.
        const submitCalls = captured.filter((c) => c.urlPath.includes('/contribution/'));
        assert.ok(submitCalls.length >= 2);
        for (const c of submitCalls) {
            assert.strictEqual(c.contributor, '0xOWNER1', 'contributor attribution');
            assert.strictEqual(c.station_id, 'PWS-OWNER1', 'station attribution');
        }
        try { require('fs').unlinkSync(tmpState); } catch (_) {}
        try { require('fs').unlinkSync(wmPath); } catch (_) {}
    });

    // 5b. commit-authorization signing (#1848/#1967): every ContributionCommit
    //     POST must carry auth_pubkey (32-byte hex) + auth_signature (64-byte
    //     hex) over the canonical message, else the mempool rejects it.
    await test('submitContributionCommit attaches valid auth_pubkey/auth_signature', async () => {
        const crypto = require('crypto');
        let commitBody = null;
        const mockPost = async (host, port, urlPath, body) => {
            if (urlPath.includes('/contribution/commit')) commitBody = body;
            return { status: 201, data: { id: 'tx1', tx_type: 'ContributionCommit' } };
        };
        const reading = { temp_c: 20, humidity: 50, pressure_hpa: 1010 };
        const stationId = 'PWS-AUTH-1';
        const res = await pipeline.submitContributionCommit(reading, stationId, '0xAUTH', {
            host: 'h', port: 1, token: 't',
            httpPost: mockPost,
            signingSeed: 'unit-test-seed-1967',
            nonce: 'unitnonce0000000000000000000000',
            timestamp: 1750000000,
        });
        assert.strictEqual(res.ok, true, 'commit accepted');
        assert.ok(commitBody, 'commit was POSTed');
        assert.strictEqual(commitBody.commit_hash.length, 64, 'commit_hash is 64 hex chars');
        assert.strictEqual(Buffer.from(commitBody.auth_pubkey, 'hex').length, 32, 'pubkey 32 bytes');
        assert.strictEqual(Buffer.from(commitBody.auth_signature, 'hex').length, 64, 'sig 64 bytes');

        // The signature must verify over the canonical message the node rebuilds
        // (station_id, commit_id=commit_hash, contribution_type="contribution").
        const msg = pipeline.commitAuthorizationMessage(stationId, commitBody.commit_hash, 'contribution');
        const key = pipeline.keyPairFromSeed(crypto.createHash('sha256').update('unit-test-seed-1967').digest());
        assert.strictEqual(commitBody.auth_pubkey, key.pubHex, 'pubkey is the seeded collector key');
        assert.ok(
            crypto.verify(null, msg, key.publicKey, Buffer.from(commitBody.auth_signature, 'hex')),
            'auth_signature verifies over canonical message',
        );
    });

    // 5c. message bytes must match the Rust contract exactly (LE-u32 length
    // prefixes), including the v2 chain-domain fold (#2034/#2076): domain tag
    // ++ str(chain_id) ++ str(network_id) ++ str(station_id) ++ str(commit_id)
    // ++ str(contribution_type). Uses the explicit-domain variant so the
    // expected bytes don't depend on the process environment.
    await test('commitAuthorizationMessageWithDomain matches shared-protocol v2 byte layout', () => {
        const chainId = '777', networkId = 'testnet';
        const station = 'S', commitId = 'CID', ctype = 'contribution';
        const msg = pipeline.commitAuthorizationMessageWithDomain(chainId, networkId, station, commitId, ctype);
        const u32 = (n) => { const b = Buffer.allocUnsafe(4); b.writeUInt32LE(n, 0); return b; };
        const expected = Buffer.concat([
            Buffer.from('wrfcoin_commit_auth_v2'),
            u32(3), Buffer.from(chainId),
            u32(7), Buffer.from(networkId),
            u32(1), Buffer.from(station),
            u32(3), Buffer.from(commitId),
            u32(12), Buffer.from(ctype),
        ]);
        assert.ok(expected.equals(msg), 'canonical v2 message bytes match');
    });

    // 5c-ii. cross-check the chain_domain fold's encoding primitives against the
    // normative golden vector in docs/specifications/SIGNING_PREIMAGE_V2.md
    // (transparent-transfer preimage — same str()/chain_domain primitives, just
    // a different domain tag and trailing fields).
    await test('chain_domain fold matches SIGNING_PREIMAGE_V2.md golden vector encoding', () => {
        const msg = pipeline.commitAuthorizationMessageWithDomain('777', 'testnet', 'x', 'y', 'z');
        // Bytes immediately after the (differing) domain tag must equal the
        // golden vector's chain_domain segment: 03000000 373737 07000000 746573746e6574
        const afterDomain = msg.subarray('wrfcoin_commit_auth_v2'.length);
        const expectedChainDomain = Buffer.from('0300000037373707000000746573746e6574', 'hex');
        assert.ok(
            afterDomain.subarray(0, expectedChainDomain.length).equals(expectedChainDomain),
            'chain_domain bytes match the golden vector encoding',
        );
    });

    // 5c-iii. resolving from the environment (no explicit override) must default
    // to the mainnet fallback identity when WRFCOIN_CHAIN_ID/WRFCOIN_NETWORK_ID
    // are unset, mirroring crypto::domain_signing::signing_chain_id/network_id.
    await test('commitAuthorizationMessage resolves mainnet fallback identity when env unset', () => {
        const savedChain = process.env.WRFCOIN_CHAIN_ID;
        const savedNetwork = process.env.WRFCOIN_NETWORK_ID;
        delete process.env.WRFCOIN_CHAIN_ID;
        delete process.env.WRFCOIN_NETWORK_ID;
        try {
            const envResolved = pipeline.commitAuthorizationMessage('S', 'CID', 'contribution');
            const explicit = pipeline.commitAuthorizationMessageWithDomain(
                'wrfcoin-main', 'mainnet', 'S', 'CID', 'contribution',
            );
            assert.ok(envResolved.equals(explicit), 'env-unset resolution matches mainnet fallback');
        } finally {
            if (savedChain === undefined) delete process.env.WRFCOIN_CHAIN_ID; else process.env.WRFCOIN_CHAIN_ID = savedChain;
            if (savedNetwork === undefined) delete process.env.WRFCOIN_NETWORK_ID; else process.env.WRFCOIN_NETWORK_ID = savedNetwork;
        }
    });

    // 5d. stable key: a seeded key is deterministic across calls (auditable).
    await test('loadCollectorSigningKey is deterministic for a fixed seed', () => {
        const a = pipeline.loadCollectorSigningKey({ signingSeed: 'stable-seed-xyz' });
        const b = pipeline.loadCollectorSigningKey({ signingSeed: 'stable-seed-xyz' });
        assert.strictEqual(a.pubHex, b.pubHex, 'same seed -> same pubkey');
        const c = pipeline.loadCollectorSigningKey({ signingSeed: 'different-seed' });
        assert.notStrictEqual(a.pubHex, c.pubHex, 'different seed -> different pubkey');
    });

    // 6. per-owner attribution differs across two contributors (no cross-contamination)
    await test('two contributors get distinct attribution', async () => {
        const byOwner = {};
        const commitIds = new Set();
        let height = 1;
        const mockPost = async (host, port, urlPath, body) => {
            if (urlPath.includes('/contribution/commit')) {
                byOwner[body.contributor] = byOwner[body.contributor] || new Set();
                byOwner[body.contributor].add(body.station_id);
                commitIds.add(body.commit_hash);
            }
            if (urlPath === '/blocks/mine') height++;
            return { status: 201, data: { height } };
        };
        const mockGet = async () => quorumPendingResponse(commitIds, { height });
        const norm = makeHistory(1).map((r) => pipeline.normalizeReading(r, { elevation_m: 10, lat: 1, lon: 2 }));
        const tmp = path.join(require('os').tmpdir(), `pr-test2-${process.pid}-${Date.now()}.json`);
        const wm1 = pipeline.getStationWatermarkPath('STA-1', { stateFilePath: tmp });
        const wm2 = pipeline.getStationWatermarkPath('STA-2', { stateFilePath: tmp });
        try { require('fs').unlinkSync(wm1); } catch (_) {}
        try { require('fs').unlinkSync(wm2); } catch (_) {}
        await pipeline.submitStationReadings(norm, 'STA-1', '0xAAA', { host: 'h', port: 1, token: 't', httpPost: mockPost, httpGet: mockGet, stateFilePath: tmp, commitBlockHeight: 1, minerAddress: TEST_MINER_ADDRESS });
        await pipeline.submitStationReadings(norm, 'STA-2', '0xBBB', { host: 'h', port: 1, token: 't', httpPost: mockPost, httpGet: mockGet, stateFilePath: tmp, commitBlockHeight: height, minerAddress: TEST_MINER_ADDRESS });
        assert.deepStrictEqual([...byOwner['0xAAA']], ['STA-1']);
        assert.deepStrictEqual([...byOwner['0xBBB']], ['STA-2']);
        try { require('fs').unlinkSync(tmp); } catch (_) {}
        try { require('fs').unlinkSync(wm1); } catch (_) {}
        try { require('fs').unlinkSync(wm2); } catch (_) {}
    });

    // 6b. P1: missing selector is rejected before device enumeration.
    await test('fetchStationReadings refuses a missing private MAC selector', async () => {
        const station = { stationId: 'PWS-MULTI', ownerAddress: '0xMULTI', appKey: 'k', apiKey: 'k' };
        const res = await pipeline.fetchStationReadings(station, {
            sleepMs: 0,
            fetchDevices: async () => ({ status: 200, data: [
                makeDevice('AA:AA:AA:AA:AA:AA', 'first'),
                makeDevice('BB:BB:BB:BB:BB:BB', 'second'),
            ] }),
            fetchDeviceHistory: async () => ({ status: 200, data: makeHistory(4) }),
        });
        assert.strictEqual(res.ok, false, 'must not submit for ambiguous multi-device account');
        assert.strictEqual(res.reason, 'invalid_config');
    });

    // 6c. P1: a sole returned device is not an implicit selector.
    await test('fetchStationReadings does not auto-select a sole device', async () => {
        const station = { stationId: 'PWS-SOLO', ownerAddress: '0xSOLO', appKey: 'k', apiKey: 'k' };
        const res = await pipeline.fetchStationReadings(station, {
            sleepMs: 0,
            maxAgeMinutes: 100000,
            fetchDevices: async () => ({ status: 200, data: [makeDevice('CC:CC:CC:CC:CC:CC', 'solo')] }),
            fetchDeviceHistory: async () => ({ status: 200, data: makeHistory(3) }),
        });
        assert.strictEqual(res.ok, false);
        assert.strictEqual(res.reason, 'invalid_config');
    });

    // 6d. P1: provided mac with no exact match in a multi-device account is rejected
    await test('fetchStationReadings requires exact MAC match in multi-device account', async () => {
        const station = { stationId: 'PWS-NOMATCH', ownerAddress: '0xNM', appKey: 'k', apiKey: 'k', mac: '00:00:00:00:00:06', model: 'WS-5000' };
        const res = await pipeline.fetchStationReadings(station, {
            sleepMs: 0,
            fetchDevices: async () => ({ status: 200, data: [
                makeDevice('AA:AA:AA:AA:AA:AA', 'a'),
                makeDevice('BB:BB:BB:BB:BB:BB', 'b'),
            ] }),
            fetchDeviceHistory: async () => ({ status: 200, data: makeHistory(2) }),
        });
        assert.strictEqual(res.ok, false);
        assert.strictEqual(res.reason, 'device_missing');
    });

    // 6e. P2: per-station watermark skips already-submitted readings on the next run
    await test('submitStationReadings dedups via per-station watermark across runs', async () => {
        const os = require('os');
        const fs = require('fs');
        const tmpState = path.join(os.tmpdir(), `pr-watermark-${process.pid}-${Date.now()}.json`);
        const wmPath = pipeline.getStationWatermarkPath('PWS-WM', { stateFilePath: tmpState });
        try { fs.unlinkSync(wmPath); } catch (_) {}

        let commitCalls = 0;
        const commitIds = new Set();
        let height = 5;
        const mockPost = async (host, port, urlPath, body) => {
            if (urlPath.includes('/contribution/commit')) {
                commitCalls++;
                commitIds.add(body.commit_hash);
            }
            if (urlPath === '/blocks/mine') height++;
            return { status: 201, data: { height } };
        };
        const mockGet = async () => quorumPendingResponse(commitIds, { height });

        // Three readings: t-2min, t-1min, t-now (newest)
        const base = Date.now();
        const mkReading = (offsetMin) => pipeline.normalizeReading({
            dateutc: base - offsetMin * 60_000,
            date: new Date(base - offsetMin * 60_000).toISOString(),
            tempf: 50, humidity: 60, baromrelin: 29.9, baromabsin: 29.9,
            windspeedmph: 5, windgustmph: 8, winddir: 180, hourlyrainin: 0, uv: 1, solarradiation: 100,
        }, { elevation_m: 10, lat: 1, lon: 2 });
        const batch = [mkReading(2), mkReading(1), mkReading(0)];

        const opts = { host: 'h', port: 1, token: 't', httpPost: mockPost, httpGet: mockGet, stateFilePath: tmpState, commitBlockHeight: 5, minerAddress: TEST_MINER_ADDRESS };

        // First run: all three commit, watermark advances to newest.
        const r1 = await pipeline.submitStationReadings(batch, 'PWS-WM', '0xWM', opts);
        assert.strictEqual(r1.committed, 3, 'first run commits all three');
        assert.strictEqual(r1.skippedByWatermark, 0);
        const wmAfter = pipeline.readStationWatermark('PWS-WM', { stateFilePath: tmpState });
        assert.ok(wmAfter > 0, 'watermark advanced');

        // Second run with the SAME overlapping batch: all skipped, nothing committed.
        commitCalls = 0;
        const r2 = await pipeline.submitStationReadings(batch, 'PWS-WM', '0xWM', opts);
        assert.strictEqual(r2.committed, 0, 'second run commits nothing (all <= watermark)');
        assert.strictEqual(r2.skippedByWatermark, 3, 'all three skipped by watermark');
        assert.strictEqual(commitCalls, 0, 'no commit POSTs on duplicate run');

        try { fs.unlinkSync(tmpState); } catch (_) {}
        try { fs.unlinkSync(wmPath); } catch (_) {}
    });

    await test('mineBlock uses an explicit authorized miner address', async () => {
        let capturedBody = null;
        const result = await pipeline.mineBlock({
            host: 'h',
            port: 1,
            token: 't',
            minerAddress: TEST_MINER_ADDRESS,
            httpPost: async (host, port, urlPath, body) => {
                assert.strictEqual(urlPath, '/blocks/mine');
                capturedBody = body;
                return { status: 201, data: { height: 8 } };
            },
        });
        assert.strictEqual(result.status, 201);
        assert.deepStrictEqual(capturedBody, { miner_address: TEST_MINER_ADDRESS });
    });

    await test('mineBlock refuses a missing miner address before HTTP', async () => {
        const saved = process.env.WRFCOIN_MINER_ADDRESS;
        delete process.env.WRFCOIN_MINER_ADDRESS;
        let requested = false;
        try {
            const result = await pipeline.mineBlock({
                host: 'h',
                port: 1,
                token: 't',
                httpPost: async () => {
                    requested = true;
                    return { status: 201, data: { height: 8 } };
                },
            });
            assert.strictEqual(result.status, null);
            assert.strictEqual(result.data.code, 'MINER_ADDRESS_REQUIRED');
            assert.strictEqual(requested, false, 'invalid config must fail before HTTP');
        } finally {
            if (saved === undefined) delete process.env.WRFCOIN_MINER_ADDRESS;
            else process.env.WRFCOIN_MINER_ADDRESS = saved;
        }
    });

    await test('collector entrypoints reject an invalid miner before network work', async () => {
        const savedMiner = process.env.WRFCOIN_MINER_ADDRESS;
        const savedExitCode = process.exitCode;
        const savedLog = console.log;
        const savedError = console.error;
        const errors = [];
        process.env.WRFCOIN_MINER_ADDRESS = 'not-an-address';
        console.log = () => {};
        console.error = (value) => { errors.push(String(value)); };
        try {
            for (const entrypoint of [single.main, multi.main]) {
                process.exitCode = 0;
                errors.length = 0;
                await entrypoint();
                assert.strictEqual(process.exitCode, 1, 'invalid miner must set a nonzero exit code');
                assert.ok(errors.some((line) => /"reason":"invalid_config"/.test(line)));
                assert.ok(errors.some((line) => /MINER_ADDRESS_REQUIRED/.test(line)));
            }
        } finally {
            if (savedMiner === undefined) delete process.env.WRFCOIN_MINER_ADDRESS;
            else process.env.WRFCOIN_MINER_ADDRESS = savedMiner;
            process.exitCode = savedExitCode;
            console.log = savedLog;
            console.error = savedError;
        }
    });

    await test('collector summary names mining failure and never invents a mine', () => {
        assert.strictEqual(
            multi.formatContributorResultStatus({ committed: 12, failed: 0, mined: false, mineFailed: true }),
            '12 submitted / 0 submit failed / MINE FAILED',
        );
        assert.strictEqual(
            multi.formatContributorResultStatus({ committed: 0, failed: 0, mined: false, mineFailed: false }),
            '0 submitted / 0 failed / NO MINE NEEDED',
        );
        assert.strictEqual(
            multi.formatContributorResultStatus({ committed: 3, failed: 0, mined: false, mineFailed: false, withheld: true, reason: 'timeout' }),
            '3 committed / 0 failed / WITHHELD (timeout)',
        );
    });

    await test('empty provider history is one upstream_stale outcome and never submits', async () => {
        const emitted = [];
        let submitCalls = 0;
        const result = await multi.processContributor({
            stationId: 'PWS-EMPTY',
            ownerAddress: '0xOWNER',
        }, {
            dryRun: false,
            emitCycleOutcome: (line) => emitted.push(JSON.parse(line)),
            fetchStationReadings: async () => ({
                ok: true,
                normalized: [],
                stale: false,
                stationName: 'synthetic-empty',
                modelBinding: 'exact',
            }),
            submitStationReadings: async () => { submitCalls++; return {}; },
        });

        assert.strictEqual(submitCalls, 0);
        assert.strictEqual(result.cycleOutcome, 'upstream_stale');
        assert.strictEqual(result.cycleReason, 'no_provider_readings');
        assert.strictEqual(emitted.length, 1);
        assert.strictEqual(emitted[0].metric_type, 'collector_station_cycle_outcome');
        assert.strictEqual(emitted[0].outcome, 'upstream_stale');
    });

    await test('chain rejection is terminal, machine-readable, and preserves commit identity', async () => {
        const emitted = [];
        const result = await multi.processContributor({
            stationId: 'PWS-REJECTED',
            ownerAddress: '0xOWNER',
        }, {
            dryRun: false,
            emitCycleOutcome: (line) => emitted.push(JSON.parse(line)),
            fetchStationReadings: async () => ({
                ok: true,
                normalized: [{ recorded_at: '2026-08-23T00:00:00.000Z' }],
                stale: false,
                stationName: 'synthetic-rejected',
                modelBinding: 'exact',
            }),
            submitStationReadings: async () => ({
                committed: 1,
                failed: 0,
                mined: false,
                withheld: true,
                reason: 'rejected',
                pending: 1,
                quorum: { attempts: 1, commitId: 'commit-rejected-2737' },
                stationCycleOutcome: {
                    outcome: 'rejected',
                    reason: 'rejected',
                    station_id: 'PWS-REJECTED',
                },
            }),
        });

        assert.strictEqual(result.cycleOutcome, 'rejected');
        assert.strictEqual(emitted.length, 1);
        assert.strictEqual(emitted[0].attempts, 1);
        assert.strictEqual(emitted[0].pending_count, 1);
        assert.strictEqual(emitted[0].commit_fingerprints.length, 1);
        assert.match(emitted[0].commit_fingerprints[0], /^[0-9a-f]{16}$/);
        assert.ok(!JSON.stringify(emitted[0]).includes('commit-rejected-2737'));
    });

    await test('exhausted quorum polling is an explicit timeout, not false health', async () => {
        const emitted = [];
        const result = await multi.processContributor({
            stationId: 'PWS-TIMEOUT-2737',
            ownerAddress: '0xOWNER',
        }, {
            dryRun: false,
            emitCycleOutcome: (line) => emitted.push(JSON.parse(line)),
            fetchStationReadings: async () => ({
                ok: true,
                normalized: [{ recorded_at: '2026-08-23T00:00:00.000Z' }],
                stale: false,
                stationName: 'synthetic-timeout',
                modelBinding: 'exact',
            }),
            submitStationReadings: async () => ({
                committed: 10,
                failed: 0,
                mined: false,
                withheld: true,
                reason: 'timeout',
                pending: 10,
                quorum: { attempts: 30, commitId: 'commit-timeout-2737' },
                stationCycleOutcome: {
                    outcome: 'timeout',
                    reason: 'timeout',
                    station_id: 'PWS-TIMEOUT-2737',
                },
            }),
        });

        assert.strictEqual(result.cycleOutcome, 'timeout');
        assert.strictEqual(result.cycleReason, 'timeout');
        assert.strictEqual(emitted.length, 1);
        assert.strictEqual(emitted[0].attempts, 30);
        assert.strictEqual(emitted[0].pending_count, 10);
        assert.strictEqual(emitted[0].commit_count, 10);
    });

    await test('invalid pending responses and missing commits never classify as paid', () => {
        for (const reason of ['auth', 'invalid_response', 'missing_commit', 'awaiting_quorum']) {
            const classified = pipeline.stationCycleOutcomeFromSubmission({
                withheld: true,
                reason,
            }, 'PWS-TIMEOUT');
            assert.strictEqual(classified.outcome, 'timeout');
            assert.strictEqual(classified.reason, reason);
        }
    });

    await test('mine response alone is pending_reconciliation (#2828) and reconciliation classification is authoritative', () => {
        const unverified = pipeline.stationCycleOutcomeFromSubmission(
            { mined: true, committed: 1, failed: 0 },
            'PWS-PAID-2737',
        );
        assert.deepStrictEqual(unverified, {
            outcome: 'pending_reconciliation',
            reason: 'canonical_reward_unverified',
            station_id: 'PWS-PAID-2737',
        });

        const paid = pipeline.stationCycleOutcomeFromSubmission({
            canonicalStationOutcomes: [{
                outcome: 'paid',
                reason: 'canonical_positive_reward',
                reward_status: 'paid',
                reward_amount: 25,
                commit_ids: ['commit-paid-2737'],
                station_id: 'PWS-PAID-2737',
            }],
        }, 'PWS-PAID-2737');
        assert.deepStrictEqual(paid, {
            outcome: 'paid',
            reason: 'canonical_positive_reward',
            reward_status: 'paid',
            reward_amount: 25,
            commit_ids: ['commit-paid-2737'],
            station_id: 'PWS-PAID-2737',
        });

        const mismatchedStation = pipeline.stationCycleOutcomeFromSubmission({
            canonicalStationOutcomes: [{
                outcome: 'paid',
                reason: 'canonical_positive_reward',
                reward_status: 'paid',
                reward_amount: 25,
                commit_ids: ['commit-wrong-station-2737'],
                station_id: 'PWS-WRONG-2737',
            }],
        }, 'PWS-EXPECTED-2737');
        assert.notStrictEqual(mismatchedStation.outcome, 'paid');

        const finalizedZero = pipeline.stationCycleOutcomeFromSubmission({
            canonicalStationOutcomes: [{
                outcome: 'rejected',
                reason: 'canonical_non_paid',
                commit_ids: ['commit-finalized-zero-2737'],
                station_id: 'PWS-PAID-2737',
                reward_statuses: ['finalized_zero'],
            }],
        }, 'PWS-PAID-2737');
        assert.notStrictEqual(finalizedZero.outcome, 'paid');
    });

    await test('mining failures emit persistent telemetry', () => {
        const savedError = console.error;
        let line = '';
        console.error = (value) => { line = String(value); };
        try {
            pipeline.emitFailureMetric('mine_failed', { status: 500 });
        } finally {
            console.error = savedError;
        }
        const metric = JSON.parse(line);
        assert.strictEqual(metric.reason, 'mine_failed');
        assert.strictEqual(metric.severity, 'persistent');
    });

    // 6f. Once preflight has accepted the configured address, a node-side
    // producer-authority rejection must remain a visible persistent failure.
    await test('producer rejection is a visible persistent mining failure', async () => {
        const os = require('os');
        const fs = require('fs');
        const tmpState = path.join(os.tmpdir(), `pr-mine-failure-${process.pid}-${Date.now()}.json`);
        const stationId = 'PWS-MINE-AUTH';
        const wmPath = pipeline.getStationWatermarkPath(stationId, { stateFilePath: tmpState });
        const authorizedMiner = '0x680d51f7543ca70029866956376306d47ce60ed0';
        try { fs.unlinkSync(wmPath); } catch (_) {}

        const base = Date.now();
        const batch = [pipeline.normalizeReading({
            dateutc: base,
            date: new Date(base).toISOString(),
            tempf: 50,
            humidity: 60,
            baromrelin: 29.9,
            baromabsin: 29.9,
            windspeedmph: 5,
            windgustmph: 8,
            winddir: 180,
            hourlyrainin: 0,
            uv: 1,
            solarradiation: 100,
        }, { elevation_m: 10, lat: 1, lon: 2 })];

        let allowMine = false;
        let height = 5;
        const commitIds = new Set();
        const mockPost = async (host, port, urlPath, body) => {
            if (urlPath === '/blocks/mine') {
                if (!allowMine || body.miner_address !== authorizedMiner) {
                    return { status: 500, data: { error: 'producer payout mismatch' } };
                }
                height++;
                return { status: 201, data: { height } };
            }
            if (urlPath.includes('/contribution/commit')) commitIds.add(body.commit_hash);
            return { status: 201, data: { height: 7 } };
        };
        const mockGet = async () => quorumPendingResponse(commitIds, { height });
        const opts = {
            host: 'h',
            port: 1,
            token: 't',
            httpPost: mockPost,
            httpGet: mockGet,
            minerAddress: authorizedMiner,
            stateFilePath: tmpState,
            commitBlockHeight: 5,
        };

        const failedMine = await pipeline.submitStationReadings(batch, stationId, '0xOWNER', opts);
        assert.strictEqual(failedMine.committed, 1, 'preflight-valid address reached the node');
        assert.strictEqual(failedMine.mined, false);
        assert.strictEqual(failedMine.mineFailed, true, 'cycle exposes the mining failure');
        assert.strictEqual(
            pipeline.readStationWatermark(stationId, { stateFilePath: tmpState }),
            0,
            'failed mine cannot advance the watermark',
        );

        allowMine = true;
        const successfulMine = await pipeline.submitStationReadings(batch, stationId, '0xOWNER', opts);
        assert.strictEqual(successfulMine.mined, true, 'authorized miner closes the cycle');
        assert.strictEqual(successfulMine.mineFailed, false);
        assert.ok(
            pipeline.readStationWatermark(stationId, { stateFilePath: tmpState }) > 0,
            'successful mine advances the watermark',
        );

        try { fs.unlinkSync(tmpState); } catch (_) {}
        try { fs.unlinkSync(wmPath); } catch (_) {}
    });

    await test('two-phase collector mines commits before reveal and mines reveals only after quorum', async () => {
        const fs = require('fs');
        const os = require('os');
        const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'wrfcoin-2774-order-'));
        const stateFilePath = path.join(tempDir, 'pending.json');
        const events = [];
        const commitIds = new Set();
        let height = 100;
        const reading = pipeline.normalizeReading({
            dateutc: Date.now(), date: new Date().toISOString(), tempf: 60,
            humidity: 50, baromrelin: 30, baromabsin: 30,
            windspeedmph: 1, windgustmph: 2, winddir: 90,
            hourlyrainin: 0, uv: 1, solarradiation: 100,
        }, { elevation_m: 10, lat: 1, lon: 2 });
        const httpPost = async (host, port, urlPath, body) => {
            events.push(urlPath);
            if (urlPath.includes('/contribution/commit')) commitIds.add(body.commit_hash);
            if (urlPath === '/blocks/mine') height++;
            return { status: 201, data: { height } };
        };
        const httpGet = async (host, port, urlPath) => {
            const statusMatch = /^\/transactions\/commit\/([^/]+)\/status$/.exec(urlPath);
            if (statusMatch) {
                // #3270: the post-mine inclusion read sees the applied reveal.
                events.push('commit-status');
                return {
                    status: 200,
                    data: {
                        commit_id: decodeURIComponent(statusMatch[1]),
                        station_id: 'PWS-ORDER',
                        status: 'revealed_attested',
                        revealed_at: height,
                        reward_status: 'pending_quorum',
                    },
                };
            }
            events.push('/transactions/oracle/pending');
            return quorumPendingResponse(commitIds, { height });
        };
        try {
            const result = await pipeline.submitStationReadings([reading], 'PWS-ORDER', '0xOWNER', {
                host: 'h', port: 1, token: 'operator-token', httpPost, httpGet,
                stateFilePath, commitBlockHeight: 100, minerAddress: TEST_MINER_ADDRESS,
                maxPolls: 1, pollIntervalMs: 0,
            });
            assert.strictEqual(result.mined, true);
            assert.deepStrictEqual(events, [
                '/transactions/contribution/commit',
                '/blocks/mine',
                '/transactions/contribution/reveal',
                '/transactions/oracle/pending',
                '/blocks/mine',
                'commit-status',
            ]);
            assert.strictEqual(result.revealed, 1);
            assert.strictEqual(result.excluded, 0);
        } finally {
            fs.rmSync(tempDir, { recursive: true, force: true });
        }
    });

    await test('mempool attesters satisfy quorum even while has_quorum is false', async () => {
        let observedToken = null;
        const result = await pipeline.awaitAttestationQuorum(['commit-2774'], {
            host: 'h', port: 1, token: 'operator-token', maxPolls: 1, pollIntervalMs: 0,
            httpGet: async (host, port, urlPath, token) => {
                assert.strictEqual(urlPath, '/transactions/oracle/pending?limit=500');
                observedToken = token;
                return quorumPendingResponse(['commit-2774'], {
                    observation: { has_quorum: false, attesting_oracles: ['oracle-a', 'oracle-b'] },
                });
            },
        });
        assert.strictEqual(observedToken, 'operator-token', 'Reader poll carries the operator token');
        assert.strictEqual(result.ready, true, 'must use mempool-inclusive attesting identities');
        const configuredThree = quorumPendingResponse(['commit-2774'], {
            observation: { has_quorum: false, attesting_oracles: ['oracle-a', 'oracle-b'] },
            response: { effective_quorum: 3 },
        });
        const evaluatedThree = pipeline.evaluateAttestationQuorum(['commit-2774'], {
            ok: true,
            observations: configuredThree.data.observations,
            effectiveQuorum: configuredThree.data.effective_quorum,
            unrecoverable: [],
        });
        assert.strictEqual(evaluatedThree.ready, false, 'chain-configured quorum is not hard-coded to two');
    });

    await test('delayed second distinct attester withholds first poll then permits quorum', async () => {
        let calls = 0;
        const result = await pipeline.awaitAttestationQuorum(['commit-delay'], {
            host: 'h', port: 1, token: 't', maxPolls: 2, pollIntervalMs: 0,
            sleepFn: async () => {},
            httpGet: async () => {
                calls++;
                return quorumPendingResponse(['commit-delay'], {
                    observation: {
                        has_quorum: false,
                        attesting_oracles: calls === 1 ? ['oracle-a'] : ['oracle-a', 'oracle-b'],
                    },
                });
            },
        });
        assert.strictEqual(calls, 2);
        assert.strictEqual(result.ready, true);
        assert.strictEqual(result.attempts, 2);
    });

    await test('empty pending response retries before accepting distinct quorum', async () => {
        let calls = 0;
        const result = await pipeline.awaitAttestationQuorum(['commit-visibility-delay'], {
            host: 'h', port: 1, token: 't', maxPolls: 2, pollIntervalMs: 0,
            sleepFn: async () => {},
            httpGet: async () => {
                calls++;
                if (calls === 1) return quorumPendingResponse([], {});
                return quorumPendingResponse(['commit-visibility-delay'], {
                    observation: {
                        has_quorum: false,
                        attesting_oracles: ['oracle-a', 'oracle-b'],
                    },
                });
            },
        });
        assert.strictEqual(calls, 2, 'an empty first poll must consume the bounded retry budget');
        assert.strictEqual(result.ready, true);
        assert.strictEqual(result.attempts, 2);
    });

    await test('empty page preserves terminal unrecoverable status', async () => {
        let calls = 0;
        const result = await pipeline.awaitAttestationQuorum(['commit-archive-gap'], {
            host: 'h', port: 1, token: 't', maxPolls: 2, pollIntervalMs: 0,
            sleepFn: async () => {},
            httpGet: async () => {
                calls++;
                return quorumPendingResponse([], {
                    response: { unrecoverable: ['commit-archive-gap'] },
                });
            },
        });
        assert.strictEqual(calls, 1, 'unrecoverable commits must remain terminal');
        assert.strictEqual(result.ready, false);
        assert.strictEqual(result.reason, 'unrecoverable');
        assert.strictEqual(result.attempts, 1);
    });

    await test('duplicate attester identity cannot satisfy distinct quorum', async () => {
        const result = await pipeline.awaitAttestationQuorum(['commit-duplicate'], {
            host: 'h', port: 1, token: 't', maxPolls: 1, pollIntervalMs: 0,
            httpGet: async () => quorumPendingResponse(['commit-duplicate'], {
                observation: { attesting_oracles: ['oracle-a', 'oracle-a'] },
            }),
        });
        assert.strictEqual(result.ready, false);
        assert.strictEqual(result.reason, 'timeout');
    });

    await test('rejection vote fails closed without a mining request', async () => {
        let mineCalls = 0;
        const entry = pipeline.normalizePendingRevealEntry(makeAwaitingEntry('commit-rejected'));
        const result = await pipeline.mineAwaitingQuorumEntries([entry], {
            host: 'h', port: 1, token: 't', maxPolls: 1, pollIntervalMs: 0,
            minerAddress: TEST_MINER_ADDRESS,
            httpGet: async () => quorumPendingResponse(['commit-rejected'], {
                observation: { rejections: 1, attesting_oracles: ['oracle-a', 'oracle-b'] },
            }),
            httpPost: async () => { mineCalls++; return { status: 201, data: { height: 102 } }; },
        });
        assert.strictEqual(result.mined, false);
        assert.strictEqual(result.reason, 'rejected');
        assert.strictEqual(mineCalls, 0);
    });

    await test('quorum-complete reveal in ready_to_mine is mined with exact station binding', async () => {
        const fs = require('fs');
        const os = require('os');
        const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'wrfcoin-ready-to-mine-'));
        const entry = pipeline.normalizePendingRevealEntry(makeAwaitingEntry('commit-ready', 'PWS-2774'));
        const stateFilePath = path.join(tempDir, 'pending.json');
        pipeline.savePendingRevealEntries(stateFilePath, [entry]);
        let mineCalls = 0;
        try {
            const result = await pipeline.mineAwaitingQuorumEntries([entry], {
                host: 'h', port: 1, token: 't', stateFilePath, maxPolls: 1, pollIntervalMs: 0,
                minerAddress: TEST_MINER_ADDRESS,
                httpGet: async () => quorumPendingResponse([], {
                    response: {
                        ready_to_mine: [{
                            commit_id: 'commit-ready',
                            station_id: 'PWS-2774',
                            source: 'mempool',
                            attesting_oracles: ['oracle-a', 'oracle-b'],
                            approvals: 2,
                            rejections: 0,
                        }],
                    },
                }),
                httpPost: async () => { mineCalls++; return { status: 201, data: { height: 102 } }; },
            });
            assert.strictEqual(result.mined, true);
            assert.strictEqual(mineCalls, 1);
        } finally {
            fs.rmSync(tempDir, { recursive: true, force: true });
        }
    });

    await test('ready_to_mine station mismatch fails closed without mining', async () => {
        const entry = pipeline.normalizePendingRevealEntry(makeAwaitingEntry('commit-mismatch', 'PWS-EXPECTED'));
        let mineCalls = 0;
        const result = await pipeline.mineAwaitingQuorumEntries([entry], {
            host: 'h', port: 1, token: 't', maxPolls: 1, pollIntervalMs: 0,
            minerAddress: TEST_MINER_ADDRESS,
            httpGet: async () => quorumPendingResponse([], {
                response: {
                    ready_to_mine: [{
                        commit_id: 'commit-mismatch',
                        station_id: 'PWS-WRONG',
                        source: 'mempool',
                        attesting_oracles: ['oracle-a', 'oracle-b'],
                    }],
                },
            }),
            httpPost: async () => { mineCalls++; return { status: 201, data: { height: 102 } }; },
        });
        assert.strictEqual(result.mined, false);
        assert.strictEqual(result.reason, 'station_binding_mismatch');
        assert.strictEqual(mineCalls, 0);
    });

    await test('auth failure and empty pending response both fail closed', async () => {
        for (const response of [
            { status: 403, data: { error: 'forbidden' } },
            quorumPendingResponse([], {}),
        ]) {
            let mineCalls = 0;
            const entry = pipeline.normalizePendingRevealEntry(makeAwaitingEntry('commit-closed'));
            const result = await pipeline.mineAwaitingQuorumEntries([entry], {
                host: 'h', port: 1, token: 't', maxPolls: 1, pollIntervalMs: 0,
                minerAddress: TEST_MINER_ADDRESS,
                httpGet: async () => response,
                httpPost: async () => { mineCalls++; return { status: 201, data: { height: 102 } }; },
            });
            assert.strictEqual(result.mined, false);
            assert.strictEqual(mineCalls, 0);
        }
    });

    await test('quorum timeout makes zero mine calls and leaves watermark unchanged', async () => {
        const fs = require('fs');
        const os = require('os');
        const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'wrfcoin-2774-timeout-'));
        const stateFilePath = path.join(tempDir, 'pending.json');
        const entry = makeAwaitingEntry('commit-timeout', 'PWS-TIMEOUT');
        pipeline.savePendingRevealEntries(stateFilePath, [entry]);
        let mineCalls = 0;
        try {
            const result = await pipeline.settlePendingAttestationBatch({
                host: 'h', port: 1, token: 't', stateFilePath,
                currentBlockHeight: 101, maxPolls: 2, pollIntervalMs: 0,
                sleepFn: async () => {}, minerAddress: TEST_MINER_ADDRESS,
                httpGet: async () => quorumPendingResponse(['commit-timeout'], {
                    observation: { attesting_oracles: ['oracle-a'] },
                }),
                httpPost: async () => { mineCalls++; return { status: 201, data: { height: 102 } }; },
            });
            assert.strictEqual(result.withheld, true);
            assert.strictEqual(mineCalls, 0);
            assert.strictEqual(pipeline.readStationWatermark('PWS-TIMEOUT', { stateFilePath }), 0);
            assert.strictEqual(pipeline.loadPendingRevealEntries(stateFilePath).length, 1);
        } finally {
            fs.rmSync(tempDir, { recursive: true, force: true });
        }
    });

    await test('canonically finalized zero reward is terminal and does not advance paid watermark', async () => {
        const fs = require('fs');
        const os = require('os');
        const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'wrfcoin-finalized-reconcile-'));
        const stateFilePath = path.join(tempDir, 'pending.json');
        const entry = makeAwaitingEntry('commit-finalized-history', 'PWS-FINALIZED');
        pipeline.savePendingRevealEntries(stateFilePath, [entry]);
        let mineCalls = 0;
        let statusCalls = 0;
        try {
            const result = await pipeline.settlePendingAttestationBatch({
                host: 'h', port: 1, token: 'operator-token', stateFilePath,
                currentBlockHeight: 600, maxPolls: 1, pollIntervalMs: 0,
                minerAddress: TEST_MINER_ADDRESS,
                httpGet: async (host, port, urlPath, token) => {
                    assert.strictEqual(token, null, 'canonical status is a public read');
                    assert.strictEqual(urlPath, '/transactions/commit/commit-finalized-history/status');
                    statusCalls++;
                    return {
                        status: 200,
                        data: {
                            commit_id: 'commit-finalized-history',
                            station_id: 'PWS-FINALIZED',
                            revealed_at: 102,
                            reward_finalized: true,
                            reward_amount: 0,
                            reward_status: 'finalized_zero',
                            status: 'revealed_pending_attestation',
                        },
                    };
                },
                httpPost: async () => { mineCalls++; return { status: 201, data: { height: 601 } }; },
            });
            assert.strictEqual(result.reason, 'canonical_finalized');
            assert.strictEqual(result.reconciled, 1);
            assert.strictEqual(result.pending, 0);
            assert.strictEqual(statusCalls, 1);
            assert.deepStrictEqual(result.canonicalStationOutcomes, [{
                outcome: 'rejected',
                reason: 'canonical_non_paid',
                station_id: entry.station_id,
                commit_ids: [entry.commit_hash],
                reward_statuses: ['finalized_zero'],
            }]);
            assert.strictEqual(mineCalls, 0, 'canonical history reconciliation never mines');
            assert.strictEqual(pipeline.loadPendingRevealEntries(stateFilePath).length, 0);
            assert.strictEqual(
                pipeline.readStationWatermark('PWS-FINALIZED', { stateFilePath }),
                0,
                'non-paid finalization must not advance the paid watermark',
            );
            assert.strictEqual(pipeline.loadTerminalOutcomes({ stateFilePath }).length, 1);
        } finally {
            fs.rmSync(tempDir, { recursive: true, force: true });
        }
    });

    await test('only a canonically paid positive reward advances the station watermark', async () => {
        const fs = require('fs');
        const os = require('os');
        const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'wrfcoin-paid-reconcile-'));
        const stateFilePath = path.join(tempDir, 'pending.json');
        const entry = makeAwaitingEntry('commit-paid-history', 'PWS-PAID');
        pipeline.savePendingRevealEntries(stateFilePath, [entry]);
        try {
            const result = await pipeline.settlePendingAttestationBatch({
                host: 'h', port: 1, token: 't', stateFilePath, currentBlockHeight: 600,
                minerAddress: TEST_MINER_ADDRESS,
                httpGet: async () => ({
                    status: 200,
                    data: {
                        commit_id: 'commit-paid-history',
                        station_id: 'PWS-PAID',
                        reward_finalized: true,
                        reward_status: 'paid',
                        reward_amount: '25',
                        status: 'reward_finalized',
                    },
                }),
            });
            assert.strictEqual(result.reason, 'canonical_finalized');
            assert.deepStrictEqual(result.canonicalStationOutcomes, [{
                outcome: 'paid',
                reason: 'canonical_positive_reward',
                station_id: entry.station_id,
                commit_ids: [entry.commit_hash],
                reward_status: 'paid',
                reward_amount: 25,
            }]);
            assert.strictEqual(
                pipeline.readStationWatermark('PWS-PAID', { stateFilePath }),
                Date.parse(entry.reading.recorded_at),
            );
            assert.strictEqual(pipeline.loadTerminalOutcomes({ stateFilePath }).length, 0);
        } finally {
            fs.rmSync(tempDir, { recursive: true, force: true });
        }
    });

    await test('station cycle reports paid only after canonical positive reward reconciliation', async () => {
        const fs = require('fs');
        const os = require('os');
        const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'wrfcoin-2737-paid-cycle-'));
        const stateFilePath = path.join(tempDir, 'pending.json');
        const entry = makeAwaitingEntry('commit-paid-cycle-2737', 'PWS-PAID-CYCLE-2737');
        pipeline.savePendingRevealEntries(stateFilePath, [entry]);
        let mineCalls = 0;
        let statusCalls = 0;
        try {
            const result = await pipeline.submitStationReadings(
                [entry.reading],
                entry.station_id,
                entry.contributor,
                {
                    host: 'h',
                    port: 1,
                    token: 't',
                    stateFilePath,
                    minerAddress: TEST_MINER_ADDRESS,
                    httpGet: async (host, port, urlPath) => {
                        if (urlPath === '/blockchain') {
                            return { status: 200, data: { height: 600 } };
                        }
                        statusCalls++;
                        return {
                            status: 200,
                            data: {
                                commit_id: entry.commit_hash,
                                station_id: entry.station_id,
                                revealed_at: 102,
                                reward_finalized: true,
                                reward_status: 'paid',
                                reward_amount: '25',
                                status: 'revealed_reward_finalized',
                            },
                        };
                    },
                    httpPost: async () => {
                        mineCalls++;
                        return { status: 201, data: { height: 601 } };
                    },
                },
            );
            assert.strictEqual(mineCalls, 0);
            assert.strictEqual(statusCalls, 1, 'canonical status is fetched exactly once');
            assert.strictEqual(result.committed, 0);
            assert.deepStrictEqual(result.canonicalOutcome, {
                outcome: 'paid',
                reason: 'canonical_positive_reward',
                station_id: entry.station_id,
                commit_ids: [entry.commit_hash],
                reward_status: 'paid',
                reward_amount: 25,
            });
            assert.strictEqual(result.stationCycleOutcome.outcome, 'paid');
        } finally {
            fs.rmSync(tempDir, { recursive: true, force: true });
        }
    });

    await test('reveal pruned after 10,000 blocks reconciles via the durable reward_finalized form (#2801)', async () => {
        const fs = require('fs');
        const os = require('os');
        const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'wrfcoin-2801-pruned-reconcile-'));
        const stateFilePath = path.join(tempDir, 'pending.json');
        const entry = makeAwaitingEntry('commit-pruned-history', 'PWS-PRUNED');
        pipeline.savePendingRevealEntries(stateFilePath, [entry]);
        let mineCalls = 0;
        try {
            const result = await pipeline.settlePendingAttestationBatch({
                host: 'h', port: 1, token: 'operator-token', stateFilePath,
                // The reveal applied at height 102; 10,000+ blocks later the
                // canonical applied_reveals entry is pruned, so the route
                // serves the durable terminal form: revealed_at null, status
                // "reward_finalized", station identity from the reward record.
                currentBlockHeight: 10_600, maxPolls: 1, pollIntervalMs: 0,
                minerAddress: TEST_MINER_ADDRESS,
                httpGet: async (host, port, urlPath, token) => {
                    assert.strictEqual(token, null, 'canonical status is a public read');
                    assert.strictEqual(urlPath, '/transactions/commit/commit-pruned-history/status');
                    return {
                        status: 200,
                        data: {
                            commit_id: 'commit-pruned-history',
                            station_id: 'PWS-PRUNED',
                            revealed_at: null,
                            reward_finalized: true,
                            reward_status: 'paid',
                            reward_amount: 240,
                            status: 'reward_finalized',
                        },
                    };
                },
                httpPost: async () => { mineCalls++; return { status: 201, data: { height: 10_601 } }; },
            });
            assert.strictEqual(result.reason, 'canonical_finalized');
            assert.strictEqual(result.reconciled, 1);
            assert.strictEqual(result.pending, 0);
            assert.strictEqual(mineCalls, 0, 'pruned-history reconciliation never mines');
            assert.strictEqual(pipeline.loadPendingRevealEntries(stateFilePath).length, 0);
            assert.strictEqual(
                pipeline.readStationWatermark('PWS-PRUNED', { stateFilePath }),
                Date.parse(entry.reading.recorded_at),
            );
        } finally {
            fs.rmSync(tempDir, { recursive: true, force: true });
        }
    });

    await test('durable-pruned paid record with reward_amount 0 settles REJECTED — never paid, never watermarked (#2801)', async () => {
        const fs = require('fs');
        const os = require('os');
        const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'wrfcoin-2801-zero-amount-'));
        const stateFilePath = path.join(tempDir, 'pending.json');
        const terminalOutcomesPath = path.join(tempDir, 'terminal.json');
        const entry = makeAwaitingEntry('commit-pruned-zero', 'PWS-PRUNED-ZERO');
        pipeline.savePendingRevealEntries(stateFilePath, [entry]);
        let mineCalls = 0;
        try {
            const result = await pipeline.settlePendingAttestationBatch({
                host: 'h', port: 1, token: 'operator-token', stateFilePath, terminalOutcomesPath,
                currentBlockHeight: 10_600, maxPolls: 1, pollIntervalMs: 0,
                minerAddress: TEST_MINER_ADDRESS,
                // A durable pruned record that CLAIMS reward_status "paid" but
                // carries no positive amount must not become a paid outcome:
                // "paid" is only load-bearing together with reward_amount > 0.
                httpGet: async () => ({
                    status: 200,
                    data: {
                        commit_id: 'commit-pruned-zero',
                        station_id: 'PWS-PRUNED-ZERO',
                        revealed_at: null,
                        reward_finalized: true,
                        reward_status: 'paid',
                        reward_amount: 0,
                        status: 'reward_finalized',
                    },
                }),
                httpPost: async () => { mineCalls++; return { status: 201, data: { height: 10_601 } }; },
            });
            assert.strictEqual(result.reason, 'canonical_finalized');
            assert.strictEqual(result.reconciled, 1);
            assert.strictEqual(result.pending, 0);
            assert.strictEqual(mineCalls, 0);
            assert.strictEqual(pipeline.loadPendingRevealEntries(stateFilePath).length, 0);
            // The load-bearing pair: no watermark advance, and the station
            // outcome is REJECTED (canonical_non_paid), never paid.
            assert.strictEqual(pipeline.readStationWatermark('PWS-PRUNED-ZERO', { stateFilePath }), 0);
            assert.strictEqual(result.canonicalStationOutcomes.length, 1);
            assert.strictEqual(result.canonicalStationOutcomes[0].outcome, 'rejected');
            assert.strictEqual(result.canonicalStationOutcomes[0].reason, 'canonical_non_paid');
            const terminal = pipeline.loadTerminalOutcomes({ stateFilePath, terminalOutcomesPath });
            assert.strictEqual(terminal.length, 1);
        } finally {
            fs.rmSync(tempDir, { recursive: true, force: true });
        }
    });

    await test('durable reward_finalized form stays fail-closed on weakened identity or non-terminal reward state (#2801)', async () => {
        const fs = require('fs');
        const os = require('os');
        const base = {
            commit_id: 'commit-pruned-guard',
            station_id: 'PWS-PRUNED-GUARD',
            revealed_at: null,
            reward_finalized: true,
            reward_status: 'paid',
            status: 'reward_finalized',
        };
        // Each variant weakens exactly one requirement of the pruned form.
        const refusedVariants = [
            { ...base, commit_id: 'commit-somebody-else' },        // commit binding
            { ...base, station_id: 'PWS-SOMEBODY-ELSE' },          // station binding
            { ...base, station_id: '' },                            // empty station: no corroboration left
            { ...base, reward_finalized: false },                   // not finalized
            { ...base, reward_status: 'pending_quorum' },           // non-terminal reward state
            { ...base, reward_status: 'not_evaluated' },
            { ...base, status: 'reward_pending_quorum' },           // wrong terminal status
            { ...base, revealed_at: 0 },                            // neither form: not null, not >= 1
        ];
        // Within the reveal window: a weakened response never settles, and
        // the entry is retained for the next cycle (pure #2801 fail-closed).
        for (const variant of refusedVariants) {
            const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'wrfcoin-2801-guard-'));
            const stateFilePath = path.join(tempDir, 'pending.json');
            const entry = makeAwaitingEntry('commit-pruned-guard', 'PWS-PRUNED-GUARD');
            pipeline.savePendingRevealEntries(stateFilePath, [entry]);
            let mineCalls = 0;
            try {
                const result = await pipeline.settlePendingAttestationBatch({
                    host: 'h', port: 1, token: 'operator-token', stateFilePath,
                    currentBlockHeight: 150, maxPolls: 1, pollIntervalMs: 0,
                    minerAddress: TEST_MINER_ADDRESS,
                    httpGet: async (host, port, urlPath) => {
                        if (urlPath.startsWith('/transactions/commit/')) {
                            return { status: 200, data: variant };
                        }
                        return quorumPendingResponse([], {});
                    },
                    httpPost: async () => { mineCalls++; return { status: 201, data: { height: 151 } }; },
                });
                assert.strictEqual(result.withheld, true, JSON.stringify(variant));
                assert.strictEqual(mineCalls, 0);
                assert.strictEqual(pipeline.loadPendingRevealEntries(stateFilePath).length, 1, JSON.stringify(variant));
                assert.strictEqual(pipeline.readStationWatermark('PWS-PRUNED-GUARD', { stateFilePath }), 0);
            } finally {
                fs.rmSync(tempDir, { recursive: true, force: true });
            }
        }

        // Past the reveal window: the same weakened responses still never pay
        // or advance a watermark — the entry becomes a durable REJECTED
        // terminal outcome via expiry recovery (#2810) instead of being
        // silently retained forever. No false settlement either way.
        for (const variant of refusedVariants) {
            const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'wrfcoin-2801-guard-expired-'));
            const stateFilePath = path.join(tempDir, 'pending.json');
            const terminalOutcomesPath = path.join(tempDir, 'terminal.json');
            const entry = makeAwaitingEntry('commit-pruned-guard', 'PWS-PRUNED-GUARD');
            pipeline.savePendingRevealEntries(stateFilePath, [entry]);
            let mineCalls = 0;
            try {
                const result = await pipeline.settlePendingAttestationBatch({
                    host: 'h', port: 1, token: 'operator-token', stateFilePath, terminalOutcomesPath,
                    currentBlockHeight: 10_600, maxPolls: 1, pollIntervalMs: 0,
                    minerAddress: TEST_MINER_ADDRESS,
                    httpGet: async (host, port, urlPath) => {
                        if (urlPath.startsWith('/transactions/commit/')) {
                            return { status: 200, data: variant };
                        }
                        return quorumPendingResponse([], {});
                    },
                    httpPost: async () => { mineCalls++; return { status: 201, data: { height: 10_601 } }; },
                });
                assert.strictEqual(result.reason, 'expired_unrecoverable', JSON.stringify(variant));
                assert.strictEqual(result.terminal, 1, JSON.stringify(variant));
                assert.strictEqual(mineCalls, 0);
                assert.strictEqual(pipeline.loadPendingRevealEntries(stateFilePath).length, 0, JSON.stringify(variant));
                assert.strictEqual(pipeline.readStationWatermark('PWS-PRUNED-GUARD', { stateFilePath }), 0);
                const terminal = pipeline.loadTerminalOutcomes({ stateFilePath, terminalOutcomesPath });
                assert.strictEqual(terminal.length, 1, JSON.stringify(variant));
            } finally {
                fs.rmSync(tempDir, { recursive: true, force: true });
            }
        }
    });

    await test('unfinalized or mismatched canonical status cannot clear awaiting state', async () => {
        const fs = require('fs');
        const os = require('os');
        const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'wrfcoin-unfinalized-reconcile-'));
        const stateFilePath = path.join(tempDir, 'pending.json');
        const entry = makeAwaitingEntry('commit-still-pending', 'PWS-STILL-PENDING');
        pipeline.savePendingRevealEntries(stateFilePath, [entry]);
        let mineCalls = 0;
        try {
            const result = await pipeline.settlePendingAttestationBatch({
                host: 'h', port: 1, token: 'operator-token', stateFilePath,
                currentBlockHeight: 600, maxPolls: 1, pollIntervalMs: 0,
                minerAddress: TEST_MINER_ADDRESS,
                httpGet: async (host, port, urlPath) => {
                    if (urlPath.startsWith('/transactions/commit/')) {
                        return {
                            status: 200,
                            data: {
                                commit_id: 'commit-still-pending',
                                station_id: 'PWS-STILL-PENDING',
                                revealed_at: 102,
                                reward_finalized: false,
                                status: 'revealed_pending_attestation',
                            },
                        };
                    }
                    return quorumPendingResponse([], {});
                },
                httpPost: async () => { mineCalls++; return { status: 201, data: { height: 601 } }; },
            });
            assert.strictEqual(result.withheld, true);
            assert.strictEqual(mineCalls, 0);
            assert.strictEqual(pipeline.loadPendingRevealEntries(stateFilePath).length, 1);
            assert.strictEqual(pipeline.readStationWatermark('PWS-STILL-PENDING', { stateFilePath }), 0);
        } finally {
            fs.rmSync(tempDir, { recursive: true, force: true });
        }
    });

    await test('expired absent reveal becomes durable terminal outcome without advancing watermark', async () => {
        const fs = require('fs');
        const os = require('os');
        const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'wrfcoin-terminal-outcome-'));
        const stateFilePath = path.join(tempDir, 'pending.json');
        const terminalOutcomesPath = path.join(tempDir, 'terminal.json');
        const entry = makeAwaitingEntry('commit-expired', 'PWS-EXPIRED');
        pipeline.savePendingRevealEntries(stateFilePath, [entry]);
        let mineCalls = 0;
        try {
            const result = await pipeline.settlePendingAttestationBatch({
                host: 'h', port: 1, token: 't', stateFilePath, terminalOutcomesPath,
                currentBlockHeight: 250, maxPolls: 1, pollIntervalMs: 0,
                minerAddress: TEST_MINER_ADDRESS,
                httpGet: async (host, port, urlPath) => {
                    if (urlPath.startsWith('/transactions/commit/')) {
                        return { status: 404, data: { code: 'not_found' } };
                    }
                    return quorumPendingResponse([], {});
                },
                httpPost: async () => { mineCalls++; return { status: 201, data: { height: 251 } }; },
            });
            assert.strictEqual(result.reason, 'expired_unrecoverable');
            assert.strictEqual(result.terminal, 1);
            assert.strictEqual(result.pending, 0);
            assert.strictEqual(mineCalls, 0);
            assert.strictEqual(pipeline.loadPendingRevealEntries(stateFilePath).length, 0);
            assert.strictEqual(pipeline.loadTerminalOutcomes({ stateFilePath, terminalOutcomesPath }).length, 1);
            assert.strictEqual(pipeline.readStationWatermark('PWS-EXPIRED', { stateFilePath }), 0);
        } finally {
            fs.rmSync(tempDir, { recursive: true, force: true });
        }
    });

    await test('saturated pending page cannot prove an expired reveal is absent', async () => {
        const entry = pipeline.normalizePendingRevealEntry(makeAwaitingEntry('commit-off-page', 'PWS-OFF-PAGE'));
        const page = Array.from(
            { length: pipeline.PENDING_ATTESTATIONS_PAGE_LIMIT },
            (_, index) => ({ commit_id: `other-${index}` }),
        );
        const terminal = await pipeline.reconcileExpiredAwaitingEntries([entry], 250, {
            host: 'h', port: 1, token: 't',
            httpGet: async () => quorumPendingResponse([], {
                response: { observations: page },
            }),
        });
        assert.deepStrictEqual(terminal, []);
    });

    await test('mixed batch mines quorum-ready reveal after quarantining expired absent peer', async () => {
        const fs = require('fs');
        const os = require('os');
        const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'wrfcoin-mixed-recovery-'));
        const stateFilePath = path.join(tempDir, 'pending.json');
        const ready = makeAwaitingEntry('commit-ready-mixed', 'PWS-MIXED');
        const expired = makeAwaitingEntry('commit-expired-mixed', 'PWS-MIXED');
        expired.timestamp += 300;
        expired.reading.recorded_at = '2026-08-12T01:45:00.000Z';
        delete expired.reading_id;
        pipeline.savePendingRevealEntries(stateFilePath, [ready, expired]);
        let mineCalls = 0;
        try {
            const result = await pipeline.settlePendingAttestationBatch({
                host: 'h', port: 1, token: 't', stateFilePath,
                currentBlockHeight: 250, maxPolls: 1, pollIntervalMs: 0,
                minerAddress: TEST_MINER_ADDRESS,
                httpGet: async (host, port, urlPath) => {
                    if (urlPath.includes('commit-expired-mixed')) return { status: 404, data: {} };
                    if (urlPath.startsWith('/transactions/commit/')) {
                        return { status: 200, data: { commit_id: 'commit-ready-mixed', reward_finalized: false } };
                    }
                    return quorumPendingResponse([], {
                        response: {
                            ready_to_mine: [{
                                commit_id: 'commit-ready-mixed',
                                station_id: 'PWS-MIXED',
                                source: 'mempool',
                                approvals: 2,
                                rejections: 0,
                                attesting_oracles: ['oracle-a', 'oracle-b'],
                            }],
                        },
                    });
                },
                httpPost: async () => { mineCalls++; return { status: 201, data: { height: 251 } }; },
            });
            assert.strictEqual(result.mined, true);
            assert.strictEqual(mineCalls, 1);
            assert.strictEqual(pipeline.loadPendingRevealEntries(stateFilePath).length, 0);
            const outcomes = pipeline.loadTerminalOutcomes({ stateFilePath });
            assert.deepStrictEqual(outcomes.map((item) => item.commit_hash), ['commit-expired-mixed']);
        } finally {
            fs.rmSync(tempDir, { recursive: true, force: true });
        }
    });

    await test('restart resumes awaiting quorum without duplicate commit and advances watermark only after reveal mine', async () => {
        const fs = require('fs');
        const os = require('os');
        const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'wrfcoin-2774-restart-'));
        const stateFilePath = path.join(tempDir, 'pending.json');
        pipeline.savePendingRevealEntries(stateFilePath, [makeAwaitingEntry('commit-restart', 'PWS-RESTART')]);
        let commitCalls = 0;
        let mineCalls = 0;
        try {
            const first = await pipeline.settlePendingAttestationBatch({
                host: 'h', port: 1, token: 't', stateFilePath,
                currentBlockHeight: 101, maxPolls: 1, pollIntervalMs: 0,
                minerAddress: TEST_MINER_ADDRESS,
                httpGet: async () => quorumPendingResponse(['commit-restart'], {
                    observation: { attesting_oracles: ['oracle-a'] },
                }),
                httpPost: async (host, port, urlPath) => {
                    if (urlPath.includes('/contribution/commit')) commitCalls++;
                    if (urlPath === '/blocks/mine') mineCalls++;
                    return { status: 201, data: { height: 102 } };
                },
            });
            assert.strictEqual(first.withheld, true);
            assert.strictEqual(pipeline.readStationWatermark('PWS-RESTART', { stateFilePath }), 0);

            const second = await pipeline.settlePendingAttestationBatch({
                host: 'h', port: 1, token: 't', stateFilePath,
                currentBlockHeight: 101, maxPolls: 1, pollIntervalMs: 0,
                minerAddress: TEST_MINER_ADDRESS,
                httpGet: async () => quorumPendingResponse(['commit-restart'], {
                    observation: { has_quorum: false, attesting_oracles: ['oracle-a', 'oracle-b'] },
                }),
                httpPost: async (host, port, urlPath) => {
                    if (urlPath.includes('/contribution/commit')) commitCalls++;
                    if (urlPath === '/blocks/mine') mineCalls++;
                    return { status: 201, data: { height: 102 } };
                },
            });
            assert.strictEqual(second.mined, true);
            assert.strictEqual(commitCalls, 0, 'restart never repeats the commit');
            assert.strictEqual(mineCalls, 1, 'only the quorum-qualified reveal block mines');
            assert.strictEqual(pipeline.loadPendingRevealEntries(stateFilePath).length, 0);
            assert.ok(pipeline.readStationWatermark('PWS-RESTART', { stateFilePath }) > 0);
        } finally {
            fs.rmSync(tempDir, { recursive: true, force: true });
        }
    });

    // 6g. P2: --dry-run propagates into the single-account fallback (no submission)
    await test('fallback honors --dry-run via WRFCOIN_FALLBACK_ARGV (no submit path)', () => {
        const saved = process.env.WRFCOIN_FALLBACK_ARGV;
        process.env.WRFCOIN_FALLBACK_ARGV = JSON.stringify(['--dry-run', '--max-age-minutes', '30']);
        // Clear the module cache so it re-parses config with the override argv.
        const singlePath = require.resolve('./fetch-ambient-weather.cjs');
        delete require.cache[singlePath];
        const single = require('./fetch-ambient-weather.cjs');
        assert.strictEqual(typeof single.main, 'function', 'single module exposes main()');
        // The dry-run flag should have been parsed from the override argv.
        assert.strictEqual(single.__dryRunForTest, true, 'fallback parsed --dry-run from override argv');
        // Restore.
        if (saved === undefined) delete process.env.WRFCOIN_FALLBACK_ARGV;
        else process.env.WRFCOIN_FALLBACK_ARGV = saved;
        delete require.cache[singlePath];
    });

    // 8. #2828 three-way cycle classification: a mined, quorum-qualified cycle
    // must never report the alarm outcome 'timeout'.
    await test('mined-but-unreconciled cycle classifies as pending_reconciliation, not timeout (#2828)', () => {
        // (a) credited-but-unreconciled: mined, no canonical entry yet.
        const unreconciled = pipeline.stationCycleOutcomeFromSubmission(
            { mined: true, canonicalStationOutcomes: [] },
            'PWS-EXAMPLE-WS5000',
        );
        assert.strictEqual(unreconciled.outcome, 'pending_reconciliation');
        assert.strictEqual(unreconciled.reason, 'canonical_reward_unverified');

        // (b) credited-and-reconciled: canonical entry wins verbatim.
        const paid = pipeline.stationCycleOutcomeFromSubmission(
            {
                mined: true,
                canonicalStationOutcomes: [{
                    station_id: 'PWS-EXAMPLE-WS5000',
                    outcome: 'paid',
                    reason: 'canonical_finalized',
                    reward_status: 'paid',
                    reward_amount: 10,
                }],
            },
            'PWS-EXAMPLE-WS5000',
        );
        assert.strictEqual(paid.outcome, 'paid');

        // (c) genuinely unpaid: the 30-poll quorum withhold stays a distinct,
        // alarming timeout — the new state must not mask it.
        const withheld = pipeline.stationCycleOutcomeFromSubmission(
            { withheld: true, reason: 'timeout', canonicalStationOutcomes: [] },
            'PWS-EXAMPLE-WS5000',
        );
        assert.strictEqual(withheld.outcome, 'timeout');

        // And a rejected settlement stays rejected.
        const rejected = pipeline.stationCycleOutcomeFromSubmission(
            { withheld: true, reason: 'rejected', canonicalStationOutcomes: [] },
            'PWS-EXAMPLE-WS5000',
        );
        assert.strictEqual(rejected.outcome, 'rejected');
    });

    await test('pending_reconciliation is emitted as a benign (stdout) station outcome (#2828)', async () => {
        const emitted = [];
        const result = await multi.processContributor({
            stationId: 'PWS-PENDING-RECON',
            ownerAddress: '0xOWNER',
        }, {
            dryRun: false,
            emitCycleOutcome: (line) => emitted.push(JSON.parse(line)),
            fetchStationReadings: async () => ({
                ok: true,
                normalized: [{ recorded_at: '2026-08-28T00:00:00.000Z' }],
                stale: false,
                stationName: 'synthetic-pending',
                modelBinding: 'exact',
            }),
            submitStationReadings: async () => {
                const result = {
                    committed: 12,
                    failed: 0,
                    mined: true,
                    withheld: false,
                    pending: 0,
                    canonicalStationOutcomes: [],
                    quorum: { attempts: 3 },
                };
                // Same wiring as the production submit path: the outcome comes
                // from the real classifier, so this test goes red if the
                // classifier regresses to 'timeout' on the mined happy path.
                return {
                    ...result,
                    stationCycleOutcome: pipeline.stationCycleOutcomeFromSubmission(
                        result,
                        'PWS-PENDING-RECON',
                    ),
                };
            },
        });
        assert.strictEqual(result.cycleOutcome, 'pending_reconciliation');
        assert.strictEqual(result.cycleReason, 'canonical_reward_unverified');
        assert.strictEqual(emitted.length, 1);
        assert.strictEqual(emitted[0].metric_type, 'collector_station_cycle_outcome');
        assert.strictEqual(emitted[0].outcome, 'pending_reconciliation');
    });

    // 7. redactSecrets scrubs Ambient key query params
    await test('redactSecrets scrubs apiKey/appKey query params', () => {
        const dirty = 'GET https://rt.ambientweather.net/v1/devices?applicationKey=abc123SECRET&apiKey=def456SECRET';
        const clean = pipeline.redactSecrets(dirty);
        assert.ok(!clean.includes('abc123SECRET'), 'appKey leaked');
        assert.ok(!clean.includes('def456SECRET'), 'apiKey leaked');
        assert.ok(clean.includes('applicationKey=<REDACTED>') || clean.includes('<TOKEN_REDACTED>'));
    });

    // 9. dashboard-visible per-station file writes (infra#897) — the whole
    // point: node-dashboard.py discovers ambient-weather-*-latest.json by
    // glob (infra#895); before this, the multi-tenant collector wrote
    // neither that file nor its accuracy counterpart, so a station
    // monitored only through this collector never appeared on the
    // dashboard even though it was being collected and paid.
    await test('stationOutputKey derives a stable, filesystem-safe slug from a stationId', () => {
        assert.strictEqual(multi.stationOutputKey('PWS-EXAMPLE-AMBIENT-01'), 'pws-example-ambient-01');
        assert.strictEqual(multi.stationOutputKey('  weird//chars!!'), 'weird-chars');
        assert.strictEqual(multi.stationOutputKey(''), 'unknown');
    });

    await test('writeStationDashboardFiles writes latest.json with the same shape fetch-ambient-weather.cjs uses for Jon', () => {
        const key = 'test-infra897-latest-only';
        const latestPath = path.join(multi.OUTPUT_DIR, `ambient-weather-${key}-latest.json`);
        const accuracyPath = path.join(multi.OUTPUT_DIR, `ambient-weather-${key}-accuracy.json`);
        try {
            fs.rmSync(latestPath, { force: true });
            fs.rmSync(accuracyPath, { force: true });

            multi.writeStationDashboardFiles(
                { stationId: key, ownerAddress: '0xOWNER' },
                {
                    stationMeta: { mac: '11:22:33:44:55:66', device_model: 'WS-TEST', location: 'Testville', lat: 1, lon: 2, elevation_m: 3 },
                    stationName: 'Test Station',
                    rawCount: 2,
                    normalized: [{ recorded_at: '2026-08-31T00:00:00.000Z', temperature_c: 20 }],
                },
                null, // NWS unavailable this cycle -- accuracy.json must be skipped, not crash
            );

            assert.ok(fs.existsSync(latestPath), 'latest.json must be written');
            const latest = JSON.parse(fs.readFileSync(latestPath, 'utf8'));
            assert.strictEqual(latest.station.name, 'Test Station');
            assert.strictEqual(latest.station.mac, '11:22:33:44:55:66');
            assert.strictEqual(latest.raw_count, 2);
            assert.strictEqual(latest.normalized.length, 1);
            assert.ok(latest.fetched_at, 'fetched_at must be present');

            assert.ok(!fs.existsSync(accuracyPath), 'accuracy.json must NOT be written when NWS is unavailable this cycle');
        } finally {
            fs.rmSync(latestPath, { force: true });
            fs.rmSync(accuracyPath, { force: true });
        }
    });

    await test('writeStationDashboardFiles writes accuracy.json when NWS observations are available', () => {
        const key = 'test-infra897-with-accuracy';
        const latestPath = path.join(multi.OUTPUT_DIR, `ambient-weather-${key}-latest.json`);
        const accuracyPath = path.join(multi.OUTPUT_DIR, `ambient-weather-${key}-accuracy.json`);
        try {
            fs.rmSync(latestPath, { force: true });
            fs.rmSync(accuracyPath, { force: true });

            const nowMs = Date.now();
            multi.writeStationDashboardFiles(
                { stationId: key, ownerAddress: '0xOWNER' },
                {
                    stationMeta: { mac: '11:22:33:44:55:66', device_model: 'WS-TEST', location: 'Testville', lat: 1, lon: 2, elevation_m: 3 },
                    stationName: 'Test Station',
                    rawCount: 1,
                    normalized: [{ recorded_at: new Date(nowMs).toISOString(), temperature_c: 20, pressure_hpa: 1013, humidity_pct: 50, wind_speed_ms: 2 }],
                },
                [{ timestamp_ms: nowMs, temperature_c: 20, pressure_hpa: 1013, humidity_pct: 50, wind_speed_ms: 2 }],
            );

            assert.ok(fs.existsSync(accuracyPath), 'accuracy.json must be written when NWS observations are supplied');
            const accuracy = JSON.parse(fs.readFileSync(accuracyPath, 'utf8'));
            assert.strictEqual(accuracy.pws_station, key);
            assert.strictEqual(accuracy.nws_station, 'KDLH');
            assert.ok(accuracy.metrics, 'metrics must be present');
        } finally {
            fs.rmSync(latestPath, { force: true });
            fs.rmSync(accuracyPath, { force: true });
        }
    });

    await test('processContributor writes dashboard files for a real (mocked-fetch) station, live mode', async () => {
        const key = 'test-infra897-via-processcontributor';
        const latestPath = path.join(multi.OUTPUT_DIR, `ambient-weather-${key}-latest.json`);
        try {
            fs.rmSync(latestPath, { force: true });
            await multi.processContributor({
                stationId: key,
                ownerAddress: '0xOWNER',
            }, {
                dryRun: false,
                nwsObs: null,
                emitCycleOutcome: () => {},
                fetchStationReadings: async () => ({
                    ok: true,
                    normalized: [{ recorded_at: new Date().toISOString(), temperature_c: 21 }],
                    stale: false,
                    stationName: 'Via processContributor',
                    modelBinding: 'exact',
                    rawCount: 1,
                }),
                submitStationReadings: async () => ({
                    committed: 1,
                    failed: 0,
                    stationCycleOutcome: { outcome: 'paid', reason: 'ok', station_id: key },
                }),
            });
            assert.ok(fs.existsSync(latestPath), 'processContributor must write latest.json, not just the standalone helper');
            const latest = JSON.parse(fs.readFileSync(latestPath, 'utf8'));
            assert.strictEqual(latest.station.name, 'Via processContributor');
        } finally {
            fs.rmSync(latestPath, { force: true });
        }
    });

    await test('processContributor writes dashboard files even in --dry-run, matching fetch-ambient-weather.cjs', async () => {
        const key = 'test-infra897-dryrun';
        const latestPath = path.join(multi.OUTPUT_DIR, `ambient-weather-${key}-latest.json`);
        try {
            fs.rmSync(latestPath, { force: true });
            let submitCalls = 0;
            await multi.processContributor({
                stationId: key,
                ownerAddress: '0xOWNER',
            }, {
                dryRun: true,
                nwsObs: null,
                fetchStationReadings: async () => ({
                    ok: true,
                    normalized: [{ recorded_at: new Date().toISOString(), temperature_c: 21 }],
                    stale: false,
                    stationName: 'Dry Run Station',
                    modelBinding: 'exact',
                    rawCount: 1,
                }),
                submitStationReadings: async () => { submitCalls++; return {}; },
            });
            assert.strictEqual(submitCalls, 0, 'dry-run must never submit');
            assert.ok(fs.existsSync(latestPath), 'dry-run must still write the dashboard file -- the fetch already happened');
        } finally {
            fs.rmSync(latestPath, { force: true });
        }
    });

    // ---------------------------------------------------------------------
    // #3266: a pending reveal that can never reach quorum must reach a
    // terminal outcome and must not withhold the cycle's fresh readings.
    // Reproduces the 2026-09-24 weather-collector-multi wedge.
    // ---------------------------------------------------------------------
    const STALE_3266_COMMIT = '580c4ef01f246ec308c00fe1a6a7be065ff46349fe921cbb962055ee72ea2742'; // pragma: allowlist secret — public on-chain commit id from the 2026-09-24 incident
    const STALE_3266_STATION = 'PWS-EXAMPLE-AMBIENT-01';
    // Reading captured 2026-09-22T21:25:00Z (244s before the 21:29:04Z
    // attestation), ~30h before the 2026-09-24T03:35Z recovery.
    const STALE_3266_READING_SECS = Date.parse('2026-09-22T21:25:00.000Z') / 1000;

    function makeStale3266Entry(overrides = {}) {
        return {
            commit_hash: STALE_3266_COMMIT,
            nonce: 'ffeeddccbbaa99887766554433221100', // pragma: allowlist secret — deterministic test fixture
            reading: {
                recorded_at: new Date(STALE_3266_READING_SECS * 1000).toISOString(),
                temperature_c: 14.2,
            },
            commit_block_height: 21104,
            station_id: STALE_3266_STATION,
            contributor: '0x465230af3383c16d024c5f4b87ed2d1b595be148',
            timestamp: STALE_3266_READING_SECS,
            quality_tier: 'good',
            phase: pipeline.PENDING_PHASE_AWAITING_QUORUM,
            reveal_submitted_at_height: 21105,
            ...overrides,
        };
    }

    // The exact body the N5 node served for 580c4ef0 after recovery
    // (GET /transactions/commit/580c4ef0…/status, 2026-09-24).
    const LIVE_3266_STATUS_BODY = {
        commit_id: STALE_3266_COMMIT,
        station_id: null,
        contribution_type: null,
        committed_at: null,
        commit_expires_at: null,
        reveal_expires_at: null,
        revealed_at: null,
        provenance_class: null,
        reward_amount: null,
        reward_finalized: false,
        reward_status: 'not_evaluated',
        approvals: 0,
        rejections: 1,
        has_quorum: false,
        oracle_quorum: 2,
        status: 'attested_without_pending_records',
    };

    function captureMetricLines() {
        const lines = [];
        const original = { log: console.log, error: console.error };
        const grab = (sink) => (...args) => {
            const text = args.map(String).join(' ');
            try {
                const parsed = JSON.parse(text);
                if (parsed && typeof parsed.metric_type === 'string') lines.push(parsed);
            } catch (_) { /* not a metric line */ }
            sink(...args);
        };
        console.log = grab(original.log);
        console.error = grab(original.error);
        return { lines, restore: () => { console.log = original.log; console.error = original.error; } };
    }

    // A mock node for one 09-24-shaped cycle: fresh commits/reveals admit
    // and gain a 2-oracle quorum; the stale commit is served by `staleStatus`
    // and by `staleObservation` (null = absent from both pending lanes).
    function make3266Node({ staleStatus, staleObservation = null, height: startHeight = 21106 }) {
        const node = { height: startHeight, commitIds: new Set(), pendingGets: 0, mines: 0, posts: [] };
        node.httpPost = async (host, port, urlPath, body) => {
            node.posts.push(urlPath);
            if (urlPath === '/blocks/mine') {
                node.mines++;
                node.height++;
                return { status: 201, data: { height: node.height } };
            }
            if (urlPath.includes('/contribution/commit')) node.commitIds.add(body.commit_hash);
            return { status: 201, data: { height: node.height } };
        };
        node.httpGet = async (host, port, urlPath) => {
            if (urlPath === '/blockchain') return { status: 200, data: { height: node.height } };
            if (urlPath.startsWith('/transactions/commit/')) {
                if (urlPath.includes(STALE_3266_COMMIT)) return staleStatus;
                return { status: 404, data: { status: 'not_found' } };
            }
            node.pendingGets++;
            const response = quorumPendingResponse(node.commitIds, { height: node.height });
            if (staleObservation) {
                response.data.observations.push({
                    ...response.data.observations[0],
                    commit_id: STALE_3266_COMMIT,
                    station_id: STALE_3266_STATION,
                    timestamp: STALE_3266_READING_SECS,
                    approvals: 0,
                    rejections: 0,
                    attesting_oracles: ['oracle-a'],
                    ...staleObservation,
                });
            }
            return response;
        };
        return node;
    }

    function freshNormalizedReadings(count = 1) {
        return makeHistory(count).map((r) => pipeline.normalizeReading(r, { elevation_m: 10, lat: 1, lon: 2 }));
    }

    await test('#3266 09-24 wedge: stale rejected reveal absent from node is terminal and the cycle commits fresh readings', async () => {
        const fs = require('fs');
        const os = require('os');
        const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'wrfcoin-3266-wedge-'));
        const stateFilePath = path.join(tempDir, 'pending-reveals.json');
        const terminalOutcomesPath = path.join(tempDir, 'terminal-outcomes.json');
        pipeline.savePendingRevealEntries(stateFilePath, [makeStale3266Entry()]);
        const node = make3266Node({ staleStatus: { status: 200, data: LIVE_3266_STATUS_BODY } });
        const capture = captureMetricLines();
        let result;
        try {
            result = await pipeline.submitStationReadings(
                freshNormalizedReadings(1),
                STALE_3266_STATION,
                '0x465230af3383c16d024c5f4b87ed2d1b595be148',
                {
                    host: 'h', port: 1, token: 't', stateFilePath, terminalOutcomesPath,
                    maxPolls: 30, pollIntervalMs: 0, sleepFn: async () => {},
                    minerAddress: TEST_MINER_ADDRESS,
                    httpGet: node.httpGet,
                    httpPost: node.httpPost,
                },
            );
        } finally {
            capture.restore();
        }
        try {
            // The stale reveal no longer withholds the cycle.
            assert.strictEqual(result.withheld, false, `cycle withheld: ${result.reason}`);
            assert.strictEqual(result.committed, 1, 'fresh reading committed in the same cycle');
            assert.strictEqual(result.mined, true, 'fresh reading reached quorum and mined');
            assert.notStrictEqual(result.stationCycleOutcome.outcome, 'timeout');
            // It never burned the 30-poll budget on the stale commit.
            assert.ok(node.pendingGets <= 3, `pending polls: ${node.pendingGets}`);
            // Durable terminal outcome with its reason; pending state cleared.
            const pendingNow = pipeline.loadPendingRevealEntries(stateFilePath);
            assert.ok(!pendingNow.some((e) => e.commit_hash === STALE_3266_COMMIT), 'stale reveal left pending state');
            const terminal = pipeline.loadTerminalOutcomes({ stateFilePath, terminalOutcomesPath });
            const stale = terminal.find((item) => item.commit_hash === STALE_3266_COMMIT);
            assert.ok(stale, 'terminal-outcomes.json records the stale commit');
            assert.strictEqual(stale.outcome, 'expired_unqualified');
            assert.strictEqual(stale.reason, 'rejected_by_quorum_member');
            assert.strictEqual(stale.station_id, STALE_3266_STATION);
            // One collector_* metric line names the terminal outcome.
            const metric = capture.lines.find((line) => line.reason === 'expired_unqualified');
            assert.ok(metric, 'expired_unqualified metric line emitted');
            assert.strictEqual(metric.metric_type, 'collector_cycle_outcome');
            assert.strictEqual(metric.terminal_reason, 'rejected_by_quorum_member');
            assert.strictEqual(metric.commit_hash, STALE_3266_COMMIT);
            assert.ok(metric.reading_age_secs > 3600, 'metric carries the reading age');
        } finally {
            fs.rmSync(tempDir, { recursive: true, force: true });
        }
    });

    await test('#3266 multi collector: processContributor reports a mined outcome, not timeout, after a stale persisted reveal', async () => {
        const fs = require('fs');
        const os = require('os');
        const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'wrfcoin-3266-multi-'));
        const stateFilePath = path.join(tempDir, 'pending-reveals.json');
        pipeline.savePendingRevealEntries(stateFilePath, [makeStale3266Entry()]);
        const node = make3266Node({ staleStatus: { status: 200, data: LIVE_3266_STATUS_BODY } });
        const emitted = [];
        try {
            const outcome = await multi.processContributor({
                stationId: STALE_3266_STATION,
                ownerAddress: '0x465230af3383c16d024c5f4b87ed2d1b595be148',
            }, {
                dryRun: false,
                nwsObs: null,
                stateFilePath,
                fetchStationReadings: async () => ({
                    ok: true,
                    normalized: freshNormalizedReadings(1),
                    stale: false,
                    stationName: 'Tioga',
                    modelBinding: 'exact',
                    rawCount: 1,
                }),
                submitStationReadings: (normalized, stationId, contributor, options) =>
                    pipeline.submitStationReadings(normalized, stationId, contributor, {
                        ...options,
                        maxPolls: 30, pollIntervalMs: 0, sleepFn: async () => {},
                        minerAddress: TEST_MINER_ADDRESS,
                        httpGet: node.httpGet,
                        httpPost: node.httpPost,
                    }),
                emitCycleOutcome: (line) => emitted.push(JSON.parse(line)),
            });
            assert.strictEqual(outcome.committed, 1);
            assert.strictEqual(outcome.cycleOutcome, 'pending_reconciliation');
            assert.strictEqual(emitted.length, 1);
            assert.strictEqual(emitted[0].metric_type, 'collector_station_cycle_outcome');
            assert.notStrictEqual(emitted[0].outcome, 'timeout');
        } finally {
            fs.rmSync(path.join(multi.OUTPUT_DIR, `ambient-weather-${multi.stationOutputKey(STALE_3266_STATION)}-latest.json`), { force: true });
            fs.rmSync(tempDir, { recursive: true, force: true });
        }
    });

    await test('#3266 reveal absent from node pending/commit state (404, in reveal window) is terminal', async () => {
        const fs = require('fs');
        const os = require('os');
        const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'wrfcoin-3266-absent-'));
        const stateFilePath = path.join(tempDir, 'pending-reveals.json');
        const entry = makeAwaitingEntry('commit-3266-absent', 'PWS-3266-ABSENT');
        pipeline.savePendingRevealEntries(stateFilePath, [entry]);
        let mineCalls = 0;
        let pendingGets = 0;
        try {
            const result = await pipeline.settlePendingAttestationBatch({
                host: 'h', port: 1, token: 't', stateFilePath,
                currentBlockHeight: 102, maxPolls: 30, pollIntervalMs: 0, sleepFn: async () => {},
                minerAddress: TEST_MINER_ADDRESS,
                httpGet: async (host, port, urlPath) => {
                    if (urlPath.startsWith('/transactions/commit/')) {
                        return { status: 404, data: { status: 'not_found' } };
                    }
                    pendingGets++;
                    return quorumPendingResponse([], {});
                },
                httpPost: async () => { mineCalls++; return { status: 201, data: { height: 103 } }; },
            });
            assert.strictEqual(result.withheld, false, `withheld: ${result.reason}`);
            assert.strictEqual(result.reason, 'expired_unqualified');
            assert.strictEqual(mineCalls, 0);
            assert.strictEqual(pendingGets, 1, 'absence proven with one read, no poll budget spent');
            assert.strictEqual(pipeline.loadPendingRevealEntries(stateFilePath).length, 0);
            const [outcome] = pipeline.loadTerminalOutcomes({ stateFilePath });
            assert.strictEqual(outcome.outcome, 'expired_unqualified');
            assert.strictEqual(outcome.reason, 'absent_from_node_pending');
            assert.strictEqual(pipeline.readStationWatermark('PWS-3266-ABSENT', { stateFilePath }), 0);
        } finally {
            fs.rmSync(tempDir, { recursive: true, force: true });
        }
    });

    await test('#3266 a visible reveal a quorum member rejected is terminal without polling', async () => {
        const fs = require('fs');
        const os = require('os');
        const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'wrfcoin-3266-rejected-'));
        const stateFilePath = path.join(tempDir, 'pending-reveals.json');
        pipeline.savePendingRevealEntries(stateFilePath, [makeAwaitingEntry('commit-3266-rejected', 'PWS-3266-REJ')]);
        let pendingGets = 0;
        try {
            const result = await pipeline.settlePendingAttestationBatch({
                host: 'h', port: 1, token: 't', stateFilePath,
                currentBlockHeight: 102, maxPolls: 30, pollIntervalMs: 0, sleepFn: async () => {},
                minerAddress: TEST_MINER_ADDRESS,
                httpGet: async (host, port, urlPath) => {
                    if (urlPath.startsWith('/transactions/commit/')) return { status: 404, data: {} };
                    pendingGets++;
                    return quorumPendingResponse(['commit-3266-rejected'], {
                        observation: { rejections: 1, attesting_oracles: ['oracle-a', 'oracle-b'] },
                    });
                },
                httpPost: async () => ({ status: 201, data: { height: 103 } }),
            });
            assert.strictEqual(result.withheld, false, `withheld: ${result.reason}`);
            assert.strictEqual(pendingGets, 1);
            const [outcome] = pipeline.loadTerminalOutcomes({ stateFilePath });
            assert.strictEqual(outcome.reason, 'rejected_by_quorum_member');
            assert.strictEqual(pipeline.loadPendingRevealEntries(stateFilePath).length, 0);
        } finally {
            fs.rmSync(tempDir, { recursive: true, force: true });
        }
    });

    await test('#3266 reveal beyond the attester freshness window short of quorum is terminal; window comes from WRFCOIN_ATTESTER_MAX_AGE_SECS', async () => {
        const fs = require('fs');
        const os = require('os');
        const nowMs = Date.parse('2026-09-24T03:35:00.000Z');
        const twoHoursOld = Math.floor(nowMs / 1000) - 7200;
        const run3266Freshness = async (label) => {
            const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), `wrfcoin-3266-fresh-${label}-`));
            const stateFilePath = path.join(tempDir, 'pending-reveals.json');
            const entry = { ...makeAwaitingEntry('commit-3266-old', 'PWS-3266-OLD'), timestamp: twoHoursOld };
            delete entry.reading_id;
            pipeline.savePendingRevealEntries(stateFilePath, [entry]);
            try {
                const result = await pipeline.settlePendingAttestationBatch({
                    host: 'h', port: 1, token: 't', stateFilePath, nowMs,
                    currentBlockHeight: 102, maxPolls: 1, pollIntervalMs: 0, sleepFn: async () => {},
                    minerAddress: TEST_MINER_ADDRESS,
                    httpGet: async (host, port, urlPath) => {
                        if (urlPath.startsWith('/transactions/commit/')) {
                            return { status: 200, data: { commit_id: 'commit-3266-old', station_id: 'PWS-3266-OLD', revealed_at: null, rejections: 0, status: 'committed_pending_reveal' } };
                        }
                        return quorumPendingResponse(['commit-3266-old'], {
                            observation: { attesting_oracles: ['oracle-a'] },
                        });
                    },
                    httpPost: async () => ({ status: 201, data: { height: 103 } }),
                });
                return {
                    result,
                    pending: pipeline.loadPendingRevealEntries(stateFilePath),
                    terminal: pipeline.loadTerminalOutcomes({ stateFilePath }),
                };
            } finally {
                fs.rmSync(tempDir, { recursive: true, force: true });
            }
        };
        const saved = process.env.WRFCOIN_ATTESTER_MAX_AGE_SECS;
        try {
            delete process.env.WRFCOIN_ATTESTER_MAX_AGE_SECS;
            assert.strictEqual(pipeline.resolveAttestationFreshnessWindowSecs(), 3600);
            const stale = await run3266Freshness('default');
            assert.strictEqual(stale.result.withheld, false);
            assert.strictEqual(stale.pending.length, 0);
            assert.strictEqual(stale.terminal[0].reason, 'beyond_attestation_freshness_window');

            // Attester configured with a 3h window: a 2h-old reading can
            // still be approved, so it keeps waiting (pre-#3266 behaviour).
            process.env.WRFCOIN_ATTESTER_MAX_AGE_SECS = '10800';
            const inWindow = await run3266Freshness('configured');
            assert.strictEqual(inWindow.result.withheld, true);
            assert.strictEqual(inWindow.pending.length, 1);
            assert.strictEqual(inWindow.terminal.length, 0);
        } finally {
            if (saved === undefined) delete process.env.WRFCOIN_ATTESTER_MAX_AGE_SECS;
            else process.env.WRFCOIN_ATTESTER_MAX_AGE_SECS = saved;
        }
    });

    await test('#3266 genuinely in-flight reveal still waits; K consecutive quorum timeouts (configurable) make it terminal', async () => {
        const fs = require('fs');
        const os = require('os');
        const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'wrfcoin-3266-k-'));
        const stateFilePath = path.join(tempDir, 'pending-reveals.json');
        pipeline.savePendingRevealEntries(stateFilePath, [makeAwaitingEntry('commit-3266-k', 'PWS-3266-K')]);
        let mineCalls = 0;
        const cycle = () => pipeline.settlePendingAttestationBatch({
            host: 'h', port: 1, token: 't', stateFilePath,
            currentBlockHeight: 102, maxPolls: 2, pollIntervalMs: 0, sleepFn: async () => {},
            minerAddress: TEST_MINER_ADDRESS,
            httpGet: async (host, port, urlPath) => {
                if (urlPath.startsWith('/transactions/commit/')) {
                    return { status: 200, data: { commit_id: 'commit-3266-k', station_id: 'PWS-3266-K', revealed_at: null, rejections: 0, status: 'committed_pending_reveal' } };
                }
                return quorumPendingResponse(['commit-3266-k'], { observation: { attesting_oracles: ['oracle-a'] } });
            },
            httpPost: async () => { mineCalls++; return { status: 201, data: { height: 103 } }; },
        });
        const saved = process.env.WRFCOIN_PENDING_REVEAL_MAX_TIMEOUT_CYCLES;
        try {
            delete process.env.WRFCOIN_PENDING_REVEAL_MAX_TIMEOUT_CYCLES;
            assert.strictEqual(pipeline.resolvePendingRevealMaxTimeoutCycles(), 3);
            for (const expectedCycles of [1, 2]) {
                const result = await cycle();
                assert.strictEqual(result.withheld, true, 'in-flight reveal keeps the pre-#3266 wait');
                assert.strictEqual(result.reason, 'timeout');
                const [entry] = pipeline.loadPendingRevealEntries(stateFilePath);
                assert.strictEqual(entry.timeout_cycles, expectedCycles, 'counter is durable');
                assert.strictEqual(pipeline.loadTerminalOutcomes({ stateFilePath }).length, 0);
            }
            const third = await cycle();
            assert.strictEqual(third.withheld, false, 'K-th timeout releases the cycle');
            assert.strictEqual(pipeline.loadPendingRevealEntries(stateFilePath).length, 0);
            const [outcome] = pipeline.loadTerminalOutcomes({ stateFilePath });
            assert.strictEqual(outcome.outcome, 'expired_unqualified');
            assert.strictEqual(outcome.reason, 'unqualified_timeout_cycles');
            assert.strictEqual(mineCalls, 0, 'the unqualified reveal was never mined');

            // K is configurable.
            process.env.WRFCOIN_PENDING_REVEAL_MAX_TIMEOUT_CYCLES = '1';
            pipeline.savePendingRevealEntries(stateFilePath, [makeAwaitingEntry('commit-3266-k', 'PWS-3266-K-ONE')]);
            const once = await cycle();
            assert.strictEqual(once.withheld, false);
            assert.strictEqual(pipeline.loadPendingRevealEntries(stateFilePath).length, 0);
        } finally {
            if (saved === undefined) delete process.env.WRFCOIN_PENDING_REVEAL_MAX_TIMEOUT_CYCLES;
            else process.env.WRFCOIN_PENDING_REVEAL_MAX_TIMEOUT_CYCLES = saved;
            fs.rmSync(tempDir, { recursive: true, force: true });
        }
    });

    await test('#3266 K-cycle bound spares a batch member that is quorum-ready on the fresh read', async () => {
        const fs = require('fs');
        const os = require('os');
        const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'wrfcoin-3266-spare-'));
        const stateFilePath = path.join(tempDir, 'pending-reveals.json');
        const ready = { ...makeAwaitingEntry('commit-3266-ready', 'PWS-TEST'), timeout_cycles: 2 };
        const stuck = { ...makeAwaitingEntry('commit-3266-stuck', 'PWS-TEST'), timeout_cycles: 2, timestamp: IN_FLIGHT_READING_SECS + 60 };
        delete stuck.reading_id;
        pipeline.savePendingRevealEntries(stateFilePath, [ready, stuck]);
        try {
            const entries = pipeline.loadPendingRevealEntries(stateFilePath);
            const pendingBody = quorumPendingResponse(['commit-3266-ready']);
            pendingBody.data.observations.push({
                ...pendingBody.data.observations[0],
                commit_id: 'commit-3266-stuck',
                attesting_oracles: ['oracle-a'],
            });
            const terminal = await pipeline.recordUnqualifiedCycle(entries, 'timeout', {
                host: 'h', port: 1, token: 't', stateFilePath, maxTimeoutCycles: 3,
                httpGet: async () => pendingBody,
            });
            assert.deepStrictEqual(terminal.map((e) => e.commit_hash), ['commit-3266-stuck']);
            const [kept] = pipeline.loadPendingRevealEntries(stateFilePath);
            assert.strictEqual(kept.commit_hash, 'commit-3266-ready');
            assert.strictEqual(kept.timeout_cycles, 0);
        } finally {
            fs.rmSync(tempDir, { recursive: true, force: true });
        }
    });

    // #3270: /blocks/mine answering 201 does not prove the block carried
    // the reveal. On 2026-09-24 the producer skipped 34 attested post-halt
    // backlog reveals (SP_CONS_INV_POP_STATE_TRANSITION_FAILED) while the
    // collector logged MINED, dropped them from pending state and advanced the
    // watermark past them, so no terminal outcome was ever recorded.
    function inclusionFixture(statusByCommit) {
        const calls = { mine: 0, status: [] };
        const httpGet = async (host, port, urlPath) => {
            const match = /^\/transactions\/commit\/([^/]+)\/status$/.exec(urlPath);
            if (match) {
                const commitId = decodeURIComponent(match[1]);
                calls.status.push(commitId);
                const spec = statusByCommit[commitId];
                if (spec instanceof Error) throw spec;
                if (spec && spec.httpStatus) return { status: spec.httpStatus, data: {} };
                return { status: 200, data: { commit_id: commitId, ...spec } };
            }
            return quorumPendingResponse(Object.keys(statusByCommit));
        };
        const httpPost = async (host, port, urlPath) => {
            if (urlPath === '/blocks/mine') calls.mine++;
            return { status: 201, data: { height: 21108 } };
        };
        return { calls, httpGet, httpPost };
    }

    function withInclusionState(entries, fn) {
        const os = require('os');
        const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'wrfcoin-inclusion-'));
        const stateFilePath = path.join(tempDir, 'pending-reveals.json');
        pipeline.savePendingRevealEntries(stateFilePath, entries);
        return Promise.resolve(fn({ stateFilePath, tempDir }))
            .finally(() => fs.rmSync(tempDir, { recursive: true, force: true }));
    }

    await test('#3270 201 mine whose block skipped the reveal (commit pruned) is terminal reveal_not_included, not MINED', async () => {
        const entry = makeAwaitingEntry('commit-skipped', 'PWS-SKIP');
        await withInclusionState([entry], async ({ stateFilePath }) => {
            const { calls, httpGet, httpPost } = inclusionFixture({
                // The live 2026-09-24 status shape for the 34 lost reveals.
                'commit-skipped': {
                    station_id: null,
                    status: 'attested_without_pending_records',
                    approvals: 2,
                    has_quorum: true,
                    committed_at: null,
                    revealed_at: null,
                    reward_finalized: false,
                    reward_status: 'not_evaluated',
                },
            });
            const [loaded] = pipeline.loadPendingRevealEntries(stateFilePath);
            const result = await pipeline.mineAwaitingQuorumEntries([loaded], {
                host: 'h', port: 1, token: 't', stateFilePath, maxPolls: 1, pollIntervalMs: 0,
                minerAddress: TEST_MINER_ADDRESS, httpGet, httpPost,
            });
            assert.strictEqual(calls.mine, 1);
            assert.deepStrictEqual(calls.status, ['commit-skipped']);
            assert.strictEqual(result.mined, false, 'a block without the reveal is not a mined reveal');
            assert.strictEqual(result.reason, pipeline.REVEAL_NOT_INCLUDED);
            assert.strictEqual(result.revealed, 0);
            assert.strictEqual(result.excluded, 1);
            assert.strictEqual(pipeline.readStationWatermark('PWS-SKIP', { stateFilePath }), 0,
                'watermark must not advance past an unpaid reading');
            assert.strictEqual(pipeline.loadPendingRevealEntries(stateFilePath).length, 0);
            const terminal = pipeline.loadTerminalOutcomes({ stateFilePath });
            assert.strictEqual(terminal.length, 1);
            assert.strictEqual(terminal[0].commit_hash, 'commit-skipped');
            assert.strictEqual(terminal[0].outcome, 'reveal_not_included');
            assert.strictEqual(terminal[0].reason, 'attested_without_pending_records');
            const outcome = pipeline.stationCycleOutcomeFromSubmission(result, 'PWS-SKIP');
            assert.strictEqual(outcome.outcome, 'rejected');
            assert.strictEqual(outcome.reason, pipeline.REVEAL_NOT_INCLUDED);
        });
    });

    await test('#3270 reveal absent from the block while its commit is live stays pending, unclaimed', async () => {
        const entry = makeAwaitingEntry('commit-deferred', 'PWS-DEFER');
        await withInclusionState([entry], async ({ stateFilePath }) => {
            const { httpGet, httpPost } = inclusionFixture({
                'commit-deferred': {
                    station_id: 'PWS-DEFER',
                    status: 'committed_pending_reveal',
                    committed_at: 21107,
                    revealed_at: null,
                    reward_status: 'not_evaluated',
                },
            });
            const [loaded] = pipeline.loadPendingRevealEntries(stateFilePath);
            const result = await pipeline.mineAwaitingQuorumEntries([loaded], {
                host: 'h', port: 1, token: 't', stateFilePath, maxPolls: 1, pollIntervalMs: 0,
                minerAddress: TEST_MINER_ADDRESS, httpGet, httpPost,
            });
            assert.strictEqual(result.mined, false);
            assert.strictEqual(result.reason, pipeline.REVEAL_NOT_INCLUDED);
            assert.strictEqual(pipeline.readStationWatermark('PWS-DEFER', { stateFilePath }), 0);
            const pending = pipeline.loadPendingRevealEntries(stateFilePath);
            assert.strictEqual(pending.length, 1, 'a possibly-deferred reveal is kept for reconciliation');
            assert.strictEqual(pending[0].phase, pipeline.PENDING_PHASE_AWAITING_QUORUM);
            assert.strictEqual(pipeline.loadTerminalOutcomes({ stateFilePath }).length, 0,
                'not terminal while the commit can still be revealed');
        });
    });

    await test('#3270 mixed batch claims only the applied reveal and advances only its watermark', async () => {
        const applied = makeAwaitingEntry('commit-applied', 'PWS-APPLIED');
        const skipped = makeAwaitingEntry('commit-lost', 'PWS-LOST');
        await withInclusionState([applied, skipped], async ({ stateFilePath }) => {
            const { httpGet, httpPost } = inclusionFixture({
                'commit-applied': {
                    station_id: 'PWS-APPLIED',
                    status: 'revealed_attested',
                    revealed_at: 21108,
                    reward_status: 'pending_quorum',
                },
                'commit-lost': { station_id: null, status: 'not_found', reward_status: 'not_evaluated' },
            });
            const loaded = pipeline.loadPendingRevealEntries(stateFilePath);
            const result = await pipeline.mineAwaitingQuorumEntries(loaded, {
                host: 'h', port: 1, token: 't', stateFilePath, maxPolls: 1, pollIntervalMs: 0,
                minerAddress: TEST_MINER_ADDRESS, httpGet, httpPost,
            });
            assert.strictEqual(result.mined, true);
            assert.strictEqual(result.revealed, 1);
            assert.strictEqual(result.excluded, 1);
            assert.ok(pipeline.readStationWatermark('PWS-APPLIED', { stateFilePath }) > 0);
            assert.strictEqual(pipeline.readStationWatermark('PWS-LOST', { stateFilePath }), 0);
            assert.strictEqual(pipeline.loadPendingRevealEntries(stateFilePath).length, 0);
            assert.deepStrictEqual(
                pipeline.loadTerminalOutcomes({ stateFilePath }).map((item) => item.commit_hash),
                ['commit-lost'],
            );
        });
    });

    await test('#3270 unreadable or unbound status keeps the claim but marks it unverified', async () => {
        for (const spec of [
            new Error('ECONNRESET'),
            { httpStatus: 503 },
            { commit_id: 'some-other-commit', status: 'not_found' },
            { station_id: 'PWS-SOMEONE-ELSE', status: 'committed_pending_reveal' },
        ]) {
            const entry = makeAwaitingEntry('commit-unverified', 'PWS-UNVERIFIED');
            await withInclusionState([entry], async ({ stateFilePath }) => {
                const { httpGet, httpPost } = inclusionFixture({ 'commit-unverified': spec });
                const [loaded] = pipeline.loadPendingRevealEntries(stateFilePath);
                const result = await pipeline.mineAwaitingQuorumEntries([loaded], {
                    host: 'h', port: 1, token: 't', stateFilePath, maxPolls: 1, pollIntervalMs: 0,
                    minerAddress: TEST_MINER_ADDRESS, httpGet, httpPost,
                });
                assert.strictEqual(result.mined, true);
                assert.strictEqual(result.excluded, 0);
                assert.strictEqual(pipeline.loadTerminalOutcomes({ stateFilePath }).length, 0,
                    'no evidence of loss is never recorded as a loss');
            });
        }
    });

    await test('#3270 end to end: collector reports REJECTED, not MINED, for a skipped backlog reveal', async () => {
        const os = require('os');
        const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'wrfcoin-inclusion-e2e-'));
        const stateFilePath = path.join(tempDir, 'pending.json');
        const commitIds = new Set();
        let height = 21106;
        const reading = pipeline.normalizeReading({
            dateutc: Date.now(), date: new Date().toISOString(), tempf: 50,
            humidity: 80, baromrelin: 30, baromabsin: 30,
            windspeedmph: 1, windgustmph: 2, winddir: 90,
            hourlyrainin: 0, uv: 0, solarradiation: 0,
        }, { elevation_m: 10, lat: 1, lon: 2 });
        const httpPost = async (host, port, urlPath, body) => {
            if (urlPath.includes('/contribution/commit')) commitIds.add(body.commit_hash);
            if (urlPath === '/blocks/mine') height++;
            return { status: 201, data: { height } };
        };
        const httpGet = async (host, port, urlPath) => {
            const match = /^\/transactions\/commit\/([^/]+)\/status$/.exec(urlPath);
            if (match) {
                return {
                    status: 200,
                    data: {
                        commit_id: decodeURIComponent(match[1]),
                        station_id: null,
                        status: 'attested_without_pending_records',
                        reward_status: 'not_evaluated',
                    },
                };
            }
            return quorumPendingResponse(commitIds, { height });
        };
        const logged = [];
        const originalLog = console.log;
        const originalError = console.error;
        console.log = (...args) => { logged.push(args.join(' ')); };
        console.error = (...args) => { logged.push(args.join(' ')); };
        try {
            const result = await pipeline.submitStationReadings([reading], 'PWS-BACKLOG', '0xOWNER', {
                host: 'h', port: 1, token: 'operator-token', httpPost, httpGet,
                stateFilePath, commitBlockHeight: 21106, minerAddress: TEST_MINER_ADDRESS,
                maxPolls: 1, pollIntervalMs: 0,
            });
            console.log = originalLog;
            console.error = originalError;
            assert.strictEqual(result.mined, false);
            assert.strictEqual(result.excluded, 1);
            assert.strictEqual(result.stationCycleOutcome.outcome, 'rejected');
            assert.strictEqual(result.stationCycleOutcome.reason, pipeline.REVEAL_NOT_INCLUDED);
            assert.ok(!logged.some((line) => line.includes('MINED quorum-qualified')),
                'must not log MINED for a reveal the block did not carry');
            assert.ok(logged.some((line) => line.includes('REJECTED [PWS-BACKLOG]: 1 reveal(s) not included')));
        } finally {
            console.log = originalLog;
            console.error = originalError;
            fs.rmSync(tempDir, { recursive: true, force: true });
        }
    });

    // ---- #3285: pressure_sea_level_hpa from the REGISTERED elevation ----

    const SEA_LEVEL_STATION = {
        stationId: 'PWS-EXAMPLE-WS5000', ownerAddress: '0xJON', appKey: 'k', apiKey: 'k',
        mac: '00:00:00:00:00:07', model: 'WS-5000', latitude: 46.83, longitude: -92.04,
    };
    const seaLevelFetchOpts = (extra = {}) => ({
        readingsLimit: 2,
        maxAgeMinutes: 100000,
        sleepMs: 0,
        fetchDevices: async () => ({ status: 200, data: [makeDevice(SEA_LEVEL_STATION.mac, 'Jon')] }),
        fetchDeviceHistory: async () => ({ status: 200, data: makeHistory(2) }),
        ...extra,
    });

    await test('#3285 fetchRegisteredElevation reads elevation_meters and never guesses', async () => {
        const calls = [];
        const got = await pipeline.fetchRegisteredElevation('PWS-EXAMPLE-WS5000', {
            host: 'node', port: 8556, token: 't',
            httpGet: async (host, port, urlPath, token) => {
                calls.push({ host, port, urlPath, token });
                return { status: 200, data: { station_id: 'PWS-EXAMPLE-WS5000', elevation_meters: 227.7 } };
            },
        });
        assert.deepStrictEqual(got, { elevation_m: 227.7, reason: 'registry' });
        assert.strictEqual(calls[0].urlPath, '/transactions/station/registrations/PWS-EXAMPLE-WS5000');

        const cases = [
            [async () => ({ status: 404, data: { error: 'NOT_FOUND' } }), 'station_unregistered'],
            [async () => ({ status: 200, data: { elevation_meters: null } }), 'no_registered_elevation'],
            [async () => ({ status: 200, data: { station_id: 'x' } }), 'no_registered_elevation'],
            [async () => ({ status: 503, data: 'down' }), 'registry_unavailable'],
            [async () => { throw new Error('ECONNREFUSED'); }, 'registry_unreachable'],
        ];
        for (const [httpGet, reason] of cases) {
            const res = await pipeline.fetchRegisteredElevation('X', { httpGet });
            assert.strictEqual(res.elevation_m, null, reason);
            assert.strictEqual(res.reason, reason);
        }
    });

    await test('#3285 a registered elevation puts pressure_sea_level_hpa in every reading', async () => {
        const res = await pipeline.fetchStationReadings(SEA_LEVEL_STATION, seaLevelFetchOpts({
            lookupRegisteredElevation: async () => ({ elevation_m: 227.7, reason: 'registry' }),
        }));
        assert.ok(res.ok, `fetch failed: ${res.detail}`);
        assert.strictEqual(res.stationMeta.elevation_m, 227.7);
        assert.strictEqual(res.stationMeta.elevation_source, 'registry');
        for (const reading of res.normalized) {
            const expected = Math.round(
                require('./ambient-weather-comparison.cjs').altimeterSettingHpa(reading.pressure_hpa, 227.7) * 100,
            ) / 100;
            assert.strictEqual(reading.pressure_sea_level_hpa, expected);
            assert.strictEqual(reading.pressure_kind, 'station');
            assert.ok(reading.pressure_sea_level_hpa - reading.pressure_hpa > 25);
        }
    });

    await test('#3285 no registered elevation: no sea-level key and no default elevation', async () => {
        for (const lookup of [
            undefined,
            async () => ({ elevation_m: null, reason: 'no_registered_elevation' }),
            async () => { throw new Error('boom'); },
        ]) {
            const res = await pipeline.fetchStationReadings(
                SEA_LEVEL_STATION,
                seaLevelFetchOpts({ lookupRegisteredElevation: lookup }),
            );
            assert.ok(res.ok, `fetch failed: ${res.detail}`);
            assert.strictEqual(res.stationMeta.elevation_m, null, 'no 220 m (or any) default');
            for (const reading of res.normalized) {
                assert.ok(!('pressure_sea_level_hpa' in reading), JSON.stringify(reading));
                assert.strictEqual(reading.elevation_m, null);
            }
        }
    });

    await test('#3285 a roster-only elevation feeds the report, not the sea-level key', async () => {
        const res = await pipeline.fetchStationReadings(
            { ...SEA_LEVEL_STATION, elevation_m: 240 },
            seaLevelFetchOpts(),
        );
        assert.ok(res.ok, `fetch failed: ${res.detail}`);
        assert.strictEqual(res.stationMeta.elevation_source, 'roster');
        assert.ok(!('pressure_sea_level_hpa' in res.normalized[0]));
        assert.strictEqual(res.normalized[0].pressure_comparison_method, 'elevation_corrected_station_pressure');

        // The registry wins over the roster.
        const registered = await pipeline.fetchStationReadings(
            { ...SEA_LEVEL_STATION, elevation_m: 240 },
            seaLevelFetchOpts({ lookupRegisteredElevation: async () => ({ elevation_m: 227.7 }) }),
        );
        assert.strictEqual(registered.stationMeta.elevation_m, 227.7);
        assert.ok('pressure_sea_level_hpa' in registered.normalized[0]);
    });

    await test('#3285 the single collector elevation override is explicit only', () => {
        assert.strictEqual(single.parseElevationOverride({}), null);
        assert.strictEqual(single.parseElevationOverride({ JONS_ELEVATION_M: '' }), null);
        // infra compose passes JONS_ELEVATION_M="" when unset: that is unset
        // (the `||` rule), never a zero elevation and never a 220 fallback.
        assert.strictEqual(single.parseElevationOverride({ JONS_ELEVATION_M: '', PWS_ELEVATION_M: '' }), null);
        assert.strictEqual(single.parseElevationOverride({ JONS_ELEVATION_M: '   ' }), null);
        assert.strictEqual(single.parseElevationOverride({ JONS_ELEVATION_M: '', PWS_ELEVATION_M: '240.5' }), 240.5);
        assert.strictEqual(single.parseElevationOverride({ JONS_ELEVATION_M: 'abc' }), null);
        assert.strictEqual(single.parseElevationOverride({ JONS_ELEVATION_M: '227.7' }), 227.7);
        assert.strictEqual(single.parseElevationOverride({ PWS_ELEVATION_M: '240.5' }), 240.5);
    });

    await test('#3285 processContributor wires the registry lookup into the fetch', async () => {
        let seenLookup = null;
        await multi.processContributor(SEA_LEVEL_STATION, {
            dryRun: true,
            lookupRegisteredElevation: async (id) => ({ elevation_m: id === SEA_LEVEL_STATION.stationId ? 227.7 : null }),
            fetchStationReadings: async (station, opts) => {
                seenLookup = opts.lookupRegisteredElevation;
                return { ok: true, normalized: [], stale: false, stationName: 'jon', modelBinding: 'exact' };
            },
            submitStationReadings: async () => ({}),
        });
        assert.strictEqual(typeof seenLookup, 'function');
        assert.deepStrictEqual(await seenLookup(SEA_LEVEL_STATION.stationId), { elevation_m: 227.7 });
    });

    console.log(`\n${passed} passed`);
}

run();
