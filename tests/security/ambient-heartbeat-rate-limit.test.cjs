'use strict';
// The node's per-IP read limiter (native-chain launch_api rate_limit.rs:
// 100 unauthenticated GETs/min, sliding window, 10x with a known bearer,
// 429 + Retry-After: 60 beyond it) must not starve heartbeat-mode commit and
// inclusion polling. Live chain 888, 24 h: single collector 272 commits / 212
// mined, multi collector 279 / 12 — its larger pass (fresh commits plus stale
// entries) spent the budget in seconds and every reveal timed out.
const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const pipeline = require('./lib/ambient-pipeline.cjs');

const STATION = 'station-rate-limit';
const CONTRIBUTOR = `0x${'2'.repeat(40)}`;
const TOKEN = 'collector-test-token';
const BLOCK_MS = 77000;
const COMMIT_HEIGHT = 1095; // cycle height; the heartbeat includes commits at 1096
const WINDOW_BLOCKS = 3; // live: committed_at 1114, commit/reveal_expires_at 1117

// Fake node: canonical heights on a 77 s heartbeat, commit/reveal windows,
// and the launch_api sliding-window read limiter keyed by one client.
function makeNode({ clock, offsetMs, saturated, readLimit = 100, authMultiplier = 10, staleIds = new Set() }) {
    // saturated: the cycle's earlier reads (it runs 4-8 min live) left the
    // budget spent and spread evenly over the trailing minute, so slots free
    // one at a time instead of in a 60 s replay of one clean burst.
    const accepted = saturated
        ? Array.from({ length: readLimit }, (_, i) => -60000 + (i + 1) * (60000 / readLimit) - 1)
        : [];
    const commits = new Map();
    const stats = { reads: 0, throttled: 0, revealsAccepted: 0, revealsRefused: 0, statusTokens: new Set() };
    let seed = 7;
    const jitter = () => { seed = (seed * 1103515245 + 12345) % 2147483648; return seed % 80; };
    // Block 1095 was produced offsetMs before the cycle started.
    const height = () => COMMIT_HEIGHT + Math.floor((clock.ms + offsetMs) / BLOCK_MS);
    function addCommit(id) {
        commits.set(id, { committedAt: COMMIT_HEIGHT + 1, expiresAt: COMMIT_HEIGHT + 1 + WINDOW_BLOCKS,
            revealedAt: null });
    }
    function status(id) {
        if (staleIds.has(id)) {
            return { status: 200, data: { commit_id: id, station_id: null, status: 'not_found' } };
        }
        const commit = commits.get(id);
        const h = height();
        if (!commit || h < commit.committedAt) return { status: 404, data: { code: 'NOT_FOUND' } };
        if (commit.revealedAt !== null && h >= commit.revealedAt) {
            return { status: 200, data: { commit_id: id, station_id: STATION, status: 'revealed_attested',
                committed_at: commit.committedAt, revealed_at: commit.revealedAt,
                commit_expires_at: commit.expiresAt, reveal_expires_at: commit.expiresAt } };
        }
        if (h >= commit.expiresAt) {
            return { status: 200, data: { commit_id: id, station_id: null, status: 'not_found' } };
        }
        return { status: 200, data: { commit_id: id, station_id: STATION, status: 'committed_pending_reveal',
            committed_at: commit.committedAt, revealed_at: null,
            commit_expires_at: commit.expiresAt, reveal_expires_at: commit.expiresAt } };
    }
    async function httpGet(_host, _port, route, token) {
        stats.reads += 1;
        while (accepted.length > 0 && accepted[0] <= clock.ms - 60000) accepted.shift();
        const limit = token === TOKEN ? readLimit * authMultiplier : readLimit;
        // A 429 is answered by the middleware before any handler runs.
        if (accepted.length >= limit) {
            clock.ms += 1;
            stats.throttled += 1;
            return { status: 429, headers: { 'retry-after': '60' }, data: { error: { code: 'TOO_MANY_REQUESTS' } } };
        }
        accepted.push(clock.ms);
        clock.ms += 20 + jitter();
        if (route === '/blockchain') {
            return { status: 200, data: { height: height(), tip: `tip-${height()}` } };
        }
        const match = /^\/transactions\/commit\/([^/]+)\/status$/.exec(route);
        if (match) {
            stats.statusTokens.add(token ?? null);
            return status(decodeURIComponent(match[1]));
        }
        return { status: 404, data: {} };
    }
    async function httpPost(_host, _port, route, body) {
        clock.ms += 50;
        if (route === '/transactions/contribution/commit') return { status: 201 };
        assert.equal(route, '/transactions/contribution/reveal');
        const commit = commits.get(body.commit_hash);
        const h = height();
        if (!commit || h < commit.committedAt) { stats.revealsRefused += 1; return { status: 409, data: {} }; }
        if (h >= commit.expiresAt) { stats.revealsRefused += 1; return { status: 410, data: {} }; }
        commit.revealedAt = h + 1;
        stats.revealsAccepted += 1;
        return { status: 201, data: {} };
    }
    return { httpGet, httpPost, addCommit, height, stats };
}

async function runCycle(t, { fresh, stale, offsetMs, saturated = false, token = null, readLimit = 100 }) {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'ambient-rate-limit-'));
    t.after(() => fs.rmSync(dir, { recursive: true, force: true }));
    const stateFilePath = path.join(dir, 'pending.json');
    const wallSecs = Math.floor(Date.now() / 1000);
    const clock = { ms: 0 };
    const staleIds = new Set();
    const node = makeNode({ clock, offsetMs, saturated, readLimit, staleIds });
    const commitOne = async (ageSecs, commitBlockHeight, temperature) => {
        const timestamp = wallSecs - ageSecs;
        const reading = { recorded_at: new Date(timestamp * 1000).toISOString(), temperature_c: temperature };
        const commit = await pipeline.submitCommitForAttestation(reading, STATION, CONTRIBUTOR, {
            stateFilePath, commitBlockHeight, signingSeed: 'rate-limit-unit-test-only', timestamp,
            httpPost: node.httpPost,
        });
        assert.equal(commit.ok, true);
        return commit.commitHash;
    };
    // Earlier cycles' commits whose windows closed: the chain reports them
    // not_found, but they are younger than the abandonment age, so they stay.
    for (let i = 0; i < stale; i++) {
        staleIds.add(await commitOne(1200 + i * 300, COMMIT_HEIGHT - 9 + i, 5 + i / 10));
    }
    const freshIds = [];
    for (let i = 0; i < fresh; i++) {
        const id = await commitOne(600 - i * 300, COMMIT_HEIGHT, 8 + i / 10);
        node.addCommit(id);
        freshIds.push(id);
    }
    const result = await pipeline.settlePendingAttestationBatch({
        stateFilePath, currentBlockHeight: COMMIT_HEIGHT, collectorMine: false,
        heartbeatIntervalSeconds: 77, nowMs: wallSecs * 1000, token,
        monotonicNow: () => clock.ms,
        sleepFn: async (ms) => { clock.ms += ms; },
        httpGet: node.httpGet, httpPost: node.httpPost,
    });
    return { result, node, freshIds, stateFilePath };
}

// The cycle's phase against the heartbeat must not decide the outcome.
const OFFSETS_MS = [5000, 20000, 40000, 60000, 75000];

for (const offsetMs of OFFSETS_MS) {
    const when = `block ${offsetMs / 1000} s before the wait`;
    for (const [fresh, stale] of [[3, 8], [3, 0], [1, 0]]) {
        test(`saturated bucket, ${when}: ${fresh} fresh + ${stale} stale reveal ${fresh}/${fresh} unauthenticated`,
            async (t) => {
                const { result, node } = await runCycle(t, { fresh, stale, offsetMs, saturated: true });
                assert.ok(node.stats.throttled > 0, 'the scenario must actually hit the limiter');
                assert.equal(node.stats.revealsRefused, 0, 'no reveal before inclusion or after expiry');
                assert.equal(node.stats.revealsAccepted, fresh,
                    `reveals accepted: ${node.stats.revealsAccepted}/${fresh}`);
                assert.equal(result.mined, true);
                assert.equal(result.revealed, fresh);
                assert.ok(node.height() <= COMMIT_HEIGHT + 1 + WINDOW_BLOCKS, 'settled inside the 3-block window');
            });
    }
}

test('fresh bucket: 3 fresh + 8 stale reveal 3/3', async (t) => {
    const { result, node } = await runCycle(t, { fresh: 3, stale: 8, offsetMs: 40000 });
    assert.equal(node.stats.revealsRefused, 0);
    assert.equal(node.stats.revealsAccepted, 3);
    assert.equal(result.revealed, 3);
});

test('commit status reads carry the collector bearer token', async (t) => {
    const { result, node } = await runCycle(t, { fresh: 3, stale: 8, offsetMs: 40000, saturated: true, token: TOKEN });
    assert.deepEqual([...node.stats.statusTokens], [TOKEN]);
    assert.equal(node.stats.throttled, 0, 'the 10x authenticated budget absorbs the spent unauthenticated one');
    assert.equal(result.revealed, 3);
});

// Unit level: what a single 429 does to the two-sighting rule.
const entry = { commit_hash: 'commit-unit', station_id: 'station-unit' };
const bound = (committedAt = 10) => ({ status: 200, data: { commit_id: entry.commit_hash,
    station_id: entry.station_id, committed_at: committedAt, status: 'committed_pending_reveal',
    commit_expires_at: 20, reveal_expires_at: 20 } });
const throttled = { status: 429, headers: { 'retry-after': '60' }, data: {} };
async function waitCommits(statuses) {
    let clock = 0;
    let calls = 0;
    const sleeps = [];
    const result = await pipeline.waitForHeartbeatCommits([entry], {
        heartbeatIntervalSeconds: 30, currentBlockHeight: 10, nowMs: Date.now(),
        monotonicNow: () => clock,
        sleepFn: async (ms) => { sleeps.push(ms); clock += ms; },
        httpGet: async (_h, _p, route) => {
            if (route === '/blockchain') return { status: 200, data: { height: 11 } };
            return statuses[Math.min(calls++, statuses.length - 1)];
        },
    });
    return { included: result.included.map((item) => item.commit_hash), sleeps };
}

test('a 429 between two bound sightings is no information: the streak holds', async () => {
    const { included, sleeps } = await waitCommits([bound(), throttled, bound()]);
    assert.deepEqual(included, [entry.commit_hash]);
    assert.ok(sleeps.some((ms) => ms > 1000), 'a 429 backs off beyond the 1 s pass interval');
    assert.ok(sleeps.every((ms) => ms <= 15000), 'Retry-After: 60 is capped inside the 3-block window');
});

test('a 404 between two bound sightings still resets the streak (reorg-out)', async () => {
    const { included } = await waitCommits([bound(), { status: 404, data: {} }, bound(), bound()]);
    assert.deepEqual(included, [entry.commit_hash]);
    const reset = await waitCommits([bound(), { status: 404, data: {} }, bound(), { status: 404, data: {} }]);
    assert.deepEqual(reset.included, []);
});

test('a changed committed_at after a 429 resets the streak', async () => {
    const { included } = await waitCommits([bound(10), throttled, bound(11), { status: 404, data: {} }]);
    assert.deepEqual(included, []);
});

test('a record that keeps failing with 500 does not hold back a confirmed neighbour', async () => {
    const other = { commit_hash: 'commit-broken', station_id: 'station-unit' };
    let clock = 0;
    const result = await pipeline.waitForHeartbeatCommits([entry, other], {
        heartbeatIntervalSeconds: 30, currentBlockHeight: 10, nowMs: Date.now(),
        monotonicNow: () => clock,
        sleepFn: async (ms) => { clock += ms; },
        httpGet: async (_h, _p, route) => {
            if (route === '/blockchain') return { status: 200, data: { height: 11 } };
            return route.includes(other.commit_hash) ? { status: 500, data: {} } : bound();
        },
    });
    assert.deepEqual(result.included.map((item) => item.commit_hash), [entry.commit_hash]);
});
