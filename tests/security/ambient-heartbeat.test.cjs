'use strict';
const { test } = require('node:test');
const assert = require('node:assert/strict');
const pipeline = require('./lib/ambient-pipeline.cjs');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const http = require('node:http');
const entry = { commit_hash: 'commit-test', station_id: 'station-test' };
test('inclusion transport bounds a hung node request', async (t) => {
    const server = http.createServer(() => {});
    await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
    t.after(() => { server.closeAllConnections(); server.close(); });
    await assert.rejects(pipeline.httpGet('127.0.0.1', server.address().port, '/test', null,
        { timeoutMs: 20 }), /request timeout/);
});
const included = (phase, height = 10) => ({ status: 200, data: {
    commit_id: entry.commit_hash, station_id: entry.station_id,
    committed_at: phase === 'commit' ? height : null,
    revealed_at: phase === 'reveal' ? height : null,
    status: phase === 'commit' ? 'committed_pending_reveal' : 'revealed_attested',
} });
async function wait(phase, responses) {
    let clock = 0;
    let calls = 0;
    const result = await pipeline.waitForHeartbeatInclusion([entry], phase, {
        heartbeatIntervalSeconds: 30, monotonicNow: () => clock,
        sleepFn: async (ms) => { clock += ms; },
        httpGet: async () => responses[Math.min(calls++, responses.length - 1)],
    });
    return { result, calls, clock };
}

for (const timeoutPhase of [null, 'commit', 'reveal']) {
    test(`full batch: ${timeoutPhase || 'included'}, no collector mine, pending state preserved`, async (t) => {
        const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'ambient-heartbeat-'));
        t.after(() => fs.rmSync(dir, { recursive: true, force: true }));
        const stateFilePath = path.join(dir, 'pending.json');
        const timestamp = Math.floor(Date.now() / 1000);
        const reading = { recorded_at: new Date(timestamp * 1000).toISOString(), temperature_c: 20 };
        const commit = await pipeline.submitCommitForAttestation(reading, entry.station_id,
            `0x${'1'.repeat(40)}`, { stateFilePath, commitBlockHeight: 9,
                signingSeed: 'heartbeat-unit-test-only', timestamp,
                httpPost: async (_host, _port, route) => {
                    assert.equal(route, '/transactions/contribution/commit');
                    return { status: 201 };
                } });
        assert.equal(commit.ok, true);
        let clock = 0;
        let revealed = false;
        let commitReads = 0;
        const result = await pipeline.settlePendingAttestationBatch({
            stateFilePath, currentBlockHeight: 9, collectorMine: false,
            heartbeatIntervalSeconds: 30, monotonicNow: () => clock,
            sleepFn: async (ms) => { clock += ms; },
            httpGet: async () => {
                if (!revealed) commitReads++;
                if ((!revealed && timeoutPhase === 'commit') || (revealed && timeoutPhase === 'reveal')) {
                    return { status: 404 };
                }
                const response = included(revealed ? 'reveal' : 'commit');
                response.data.commit_id = commit.commitHash;
                return response;
            },
            httpPost: async (_host, _port, route, body) => {
                assert.equal(route, '/transactions/contribution/reveal');
                assert.ok(commitReads >= 2, 'reveal cannot precede canonical commit inclusion');
                assert.equal(body.timestamp, timestamp, 'committed timestamp must remain unchanged');
                assert.equal(body.commit_hash, commit.commitHash);
                revealed = true;
                return { status: 201 };
            },
        });
        const remaining = pipeline.loadPendingRevealEntries(stateFilePath);
        if (timeoutPhase) {
            assert.equal(result.mined, false);
            assert.equal(remaining.length, 1);
            assert.equal(pipeline.readStationWatermark(entry.station_id, { stateFilePath }), 0);
            assert.equal(remaining[0].phase, timeoutPhase === 'commit'
                ? pipeline.PENDING_PHASE_COMMITTED : pipeline.PENDING_PHASE_AWAITING_QUORUM);
        } else {
            assert.equal(result.mined, true);
            assert.equal(remaining.length, 0);
            assert.equal(pipeline.readStationWatermark(entry.station_id, { stateFilePath }), timestamp * 1000);
        }
    });
}
test('default mining stays enabled; false suppresses every direct mine', async () => {
    assert.equal(pipeline.collectorMineEnabled(), true);
    assert.equal(pipeline.collectorMineEnabled({ collectorMine: 'false' }), false);
    assert.throws(() => pipeline.collectorMineEnabled({ collectorMine: 'FALSE' }));
    const response = await pipeline.mineBlock({ collectorMine: false,
        httpPost: () => { throw new Error('must not mine'); } });
    assert.equal(response.data.code, 'COLLECTOR_MINING_DISABLED');
});
for (const phase of ['commit', 'reveal']) {
    test(`${phase}: waits for two stable canonical inclusions`, async () => {
        const { result, calls } = await wait(phase, [{ status: 404 }, included(phase), included(phase)]);
        assert.equal(result.status, 201);
        assert.equal(calls, 3);
        assert.equal(result.data.height, 10);
    });
    test(`${phase}: inclusion timeout is bounded at three intervals`, async () => {
        const { result, clock } = await wait(phase, [{ status: 404 }]);
        assert.equal(result.data.code, 'HEARTBEAT_INCLUSION_TIMEOUT');
        assert.equal(clock, 90000);
    });
    test(`${phase}: reorg resets inclusion evidence`, async () => {
        const { result, calls } = await wait(phase,
            [included(phase), { status: 404 }, included(phase, 12), included(phase, 12)]);
        assert.equal(result.status, 201);
        assert.equal(result.data.height, 12);
        assert.equal(calls, 4);
    });
    test(`${phase}: unbound status cannot prove inclusion`, async () => {
        const wrong = included(phase);
        wrong.data.station_id = 'wrong-station';
        const { result } = await wait(phase, [wrong]);
        assert.equal(result.status, null);
    });
}

for (const interval of [60, 77]) {
    for (const rollback of [false, true]) {
        test(`${interval}s: expiry reconfirms tip/status across ${rollback ? '13 -> 12 rollback' : 'stable tip 13'}`, async (t) => {
            const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'heartbeat-expiry-race-'));
            t.after(() => fs.rmSync(dir, { recursive: true, force: true }));
            const stateFilePath = path.join(dir, 'pending.json');
            const timestamp = Math.floor(Date.now() / 1000);
            pipeline.savePendingRevealEntries(stateFilePath, [{ ...entry,
                nonce: 'rollback-test', timestamp, commit_block_height: 9,
                reading: { recorded_at: new Date(timestamp * 1000).toISOString() },
                phase: pipeline.PENDING_PHASE_COMMITTED }]);
            let clock = 0;
            let tip = 13;
            let statusReads = 0;
            const result = await pipeline.waitForHeartbeatCommits(pipeline.loadPendingRevealEntries(stateFilePath), {
                stateFilePath, collectorMine: false, heartbeatIntervalSeconds: interval,
                monotonicNow: () => clock, sleepFn: async (ms) => { clock += ms; },
                httpGet: async (_host, _port, route) => {
                    if (route === '/blockchain') return { status: 200, data: { height: tip, tip: `hash-${tip}` } };
                    statusReads++;
                    if (rollback && statusReads === 1) tip = 12;
                    return { status: 200, data: { commit_id: entry.commit_hash, station_id: entry.station_id,
                        committed_at: 10, commit_expires_at: 13, reveal_expires_at: 13, revealed_at: null,
                        status: 'committed_pending_reveal' } };
                },
            });
            assert.ok(clock <= interval * 3000);
            if (rollback) {
                assert.equal(pipeline.loadTerminalOutcomes({ stateFilePath }).length, 0);
                assert.equal(pipeline.loadPendingRevealEntries(stateFilePath).length, 1);
                assert.equal(result.included.length, 1);
                assert.equal(result.height, 12);
            } else {
                assert.equal(pipeline.loadPendingRevealEntries(stateFilePath).length, 0);
                assert.equal(pipeline.loadTerminalOutcomes({ stateFilePath })[0].reason, 'commit_window_expired');
                assert.ok(statusReads >= 2, 'durable expiry requires a fresh status read');
            }
        });
    }
    for (const scenario of ['expired', 'reorged-out', 'mixed']) {
        test(`${interval}s: ${scenario} committed records cannot stall fresh work or duplicate submissions`, async (t) => {
            const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'heartbeat-stale-'));
            t.after(() => fs.rmSync(dir, { recursive: true, force: true }));
            const stateFilePath = path.join(dir, 'pending.json');
            const wall = Date.now();
            const make = (id, snapshot, age = 0) => ({
                commit_hash: id, station_id: 'station-test', contributor: `0x${'1'.repeat(40)}`,
                nonce: 'test-nonce', timestamp: Math.floor(wall / 1000) - age,
                reading: { recorded_at: new Date(wall - age * 1000).toISOString(), temperature_c: 20 },
                commit_block_height: snapshot, phase: pipeline.PENDING_PHASE_COMMITTED,
            });
            const old = make('old-commit', 9, 100);
            const fresh = make('fresh-commit', 19);
            let clock = 0;
            let revealed = false;
            const posts = [];
            const options = {
                stateFilePath, collectorMine: false, heartbeatIntervalSeconds: interval,
                currentBlockHeight: 20, nowMs: wall, monotonicNow: () => clock,
                sleepFn: async (ms) => { clock += ms; },
                httpGet: async (_host, _port, route) => {
                    if (route === '/blockchain') return { status: 200, data: { height: revealed ? 21 : 20 } };
                    const id = route.includes('old-commit') ? 'old-commit' : 'fresh-commit';
                    if (id === 'old-commit' && scenario === 'reorged-out') return { status: 404 };
                    return { status: 200, data: {
                        commit_id: id, station_id: 'station-test',
                        committed_at: id === 'old-commit' ? 10 : revealed ? null : 20,
                        revealed_at: id === 'fresh-commit' && revealed ? 21 : null,
                        commit_expires_at: id === 'old-commit' ? 13 : 23,
                        reveal_expires_at: id === 'old-commit' ? 13 : 23,
                        status: id === 'fresh-commit' && revealed ? 'revealed_attested' : 'committed_pending_reveal',
                    } };
                },
                httpPost: async (_host, _port, route, body) => {
                    posts.push({ route, id: body.commit_hash });
                    assert.equal(route, '/transactions/contribution/reveal');
                    assert.equal(body.commit_hash, 'fresh-commit');
                    revealed = true;
                    return { status: 201 };
                },
            };
            pipeline.savePendingRevealEntries(stateFilePath, scenario === 'mixed' ? [old, fresh] : [old]);
            const first = await pipeline.settlePendingAttestationBatch(options);
            if (scenario === 'mixed') {
                assert.equal(first.mined, true);
                assert.ok(clock <= 2000, 'stale neighbour must not consume the inclusion budget');
            } else {
                assert.equal(first.withheld, false, 'unresolved committed state must not block new cycles');
            }
            if (scenario === 'reorged-out') {
                assert.equal(clock, interval * 3000);
                assert.equal(pipeline.loadPendingRevealEntries(stateFilePath).length, 1);
                assert.equal(pipeline.loadTerminalOutcomes(options).length, 0, 'transient 404 is not terminal');
                // Restart after the explicit reading max-age policy. The same
                // lost packet is abandoned durably, never regenerated.
                options.nowMs = wall + 3601000;
                fresh.timestamp += 3601;
                fresh.reading.recorded_at = new Date(wall + 3601000).toISOString();
            }
            if (scenario !== 'mixed') {
                const waiting = pipeline.loadPendingRevealEntries(stateFilePath);
                pipeline.savePendingRevealEntries(stateFilePath, [...waiting, fresh]);
                const next = await pipeline.settlePendingAttestationBatch(options);
                assert.equal(next.mined, true);
            }
            assert.equal(pipeline.loadPendingRevealEntries(stateFilePath).length, 0);
            const terminal = pipeline.loadTerminalOutcomes(options);
            assert.equal(terminal.length, 1);
            assert.equal(terminal[0].commit_hash, 'old-commit');
            assert.equal(terminal[0].reason, scenario === 'reorged-out' ? 'commit_abandoned_max_age' : 'commit_window_expired');
            assert.deepEqual(posts, [{ route: '/transactions/contribution/reveal', id: 'fresh-commit' }]);
            await pipeline.settlePendingAttestationBatch(options);
            assert.equal(posts.length, 1, 'restart cannot submit the reveal twice');
            await pipeline.submitStationReadings([old.reading, fresh.reading], 'station-test', old.contributor,
                { ...options, minerAddress: old.contributor });
            assert.equal(posts.length, 1, 'terminal IDs and watermark prevent duplicate commits');
        });
    }
}
