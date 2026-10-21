'use strict';

const crypto = require('crypto');
const fs = require('fs');
const os = require('os');
const path = require('path');

const {
    computeCommitId,
    computeNextLastSubmittedTs,
    computePendingRevealReadingId,
    mineBlock,
    processPendingReveals,
    sortKeys,
    submitCommitReveal,
} = require('./fetch-ambient-weather.cjs');
const {
    keyPairFromSeed,
    signCommitAuthorization,
} = require('./lib/ambient-pipeline.cjs');

describe('fetch-ambient-weather commit/reveal helpers', () => {
    test('sortKeys produces serde_json BTreeMap-compatible ordering', () => {
        const sorted = sortKeys({
            z: 1,
            a: 2,
            nested: {
                c: 3,
                b: 4,
            },
            readings: [
                { y: 2, x: 1 },
                { b: 2, a: 1 },
            ],
        });

        expect(Object.keys(sorted)).toEqual(['a', 'nested', 'readings', 'z']);
        expect(Object.keys(sorted.nested)).toEqual(['b', 'c']);
        expect(Object.keys(sorted.readings[0])).toEqual(['x', 'y']);
        expect(Object.keys(sorted.readings[1])).toEqual(['a', 'b']);
    });

    test('computeCommitId matches the shared-protocol SHA3-256 preimage contract', () => {
        const data = {
            temperature_c: 22.1,
            humidity_pct: 48.2,
            nested: { z: 3, a: 1 },
        };
        const nonce = 'abc123nonce';
        const stationId = 'TEST-001';
        const timestamp = 1700000000;
        const contributionType = 'contribution';
        const sorted = sortKeys(data);
        const preimage =
            `${JSON.stringify(sorted)}||${nonce}||${stationId}||${timestamp}||${contributionType}`;
        const expectedHash = crypto.createHash('sha3-256').update(preimage).digest('hex');

        expect(preimage).toBe(
            '{"humidity_pct":48.2,"nested":{"a":1,"z":3},"temperature_c":22.1}||abc123nonce||TEST-001||1700000000||contribution',
        );
        expect(expectedHash).toBe('808de0547c99b747cf2dc8d96eb425770ae206ac64cb36987eabf6d2e66efe4b');
        expect(computeCommitId(data, nonce, stationId, timestamp, contributionType)).toBe(expectedHash);
    });

    test('submitCommitReveal formats commit and reveal requests for the WRFCoin API', async () => {
        const httpPost = jest
            .fn()
            .mockResolvedValueOnce({ status: 201, data: { accepted: true } })
            .mockResolvedValueOnce({ status: 200, data: { revealed: true } });
        const reading = {
            humidity_pct: 61,
            metadata: { z: 3, a: 1 },
            recorded_at: '2026-03-20T01:02:03.000Z',
            temperature_c: 4.2,
        };
        const stationId = 'PWS-TEST-001';
        const contributor = '0x1234';
        const nonceBuffer = Buffer.from('00112233445566778899aabbccddeeff', 'hex');
        const expectedPayload = sortKeys(reading);
        const expectedTimestamp = Math.floor(new Date(reading.recorded_at).getTime() / 1000);
        const expectedHash = computeCommitId(
            expectedPayload,
            nonceBuffer.toString('hex'),
            stationId,
            expectedTimestamp,
            'contribution',
        );
        const signingKey = keyPairFromSeed(Buffer.alloc(32, 0x11));
        const expectedAuth = signCommitAuthorization(
            stationId,
            expectedHash,
            'contribution',
            { signingKey },
        );

        const result = await submitCommitReveal(reading, stationId, contributor, {
            host: '127.0.0.1',
            port: 8556,
            token: 'operator-token',
            httpPost,
            randomBytes: () => nonceBuffer,
            commitBlockHeight: 42,
            signingKey,
        });

        expect(result).toEqual({
            ok: true,
            commitHash: expectedHash,
            timestamp: expectedTimestamp,
        });
        expect(httpPost).toHaveBeenCalledTimes(2);
        expect(httpPost).toHaveBeenNthCalledWith(
            1,
            '127.0.0.1',
            8556,
            '/transactions/contribution/commit',
            {
                contributor,
                station_id: stationId,
                commit_hash: expectedHash,
                ...expectedAuth,
            },
            'operator-token',
        );
        expect(httpPost).toHaveBeenNthCalledWith(
            2,
            '127.0.0.1',
            8556,
            '/transactions/contribution/reveal',
            {
                contributor,
                station_id: stationId,
                payload: expectedPayload,
                nonce: nonceBuffer.toString('hex'),
                commit_hash: expectedHash,
                timestamp: expectedTimestamp,
            },
            'operator-token',
        );
    });

    test('submitCommitReveal returns a commit-phase failure without attempting reveal', async () => {
        const httpPost = jest.fn().mockResolvedValue({
            status: 503,
            data: { error: 'commit unavailable' },
        });

        const result = await submitCommitReveal(
            { temperature_c: 1.5, humidity_pct: 80 },
            'PWS-TEST-001',
            '0x1234',
            { httpPost, randomBytes: () => Buffer.alloc(16, 0x11), commitBlockHeight: 42 },
        );

        expect(result).toEqual({
            ok: false,
            phase: 'commit',
            status: 503,
            detail: { error: 'commit unavailable' },
        });
        expect(httpPost).toHaveBeenCalledTimes(1);
    });

    test('submitCommitReveal returns a reveal-phase failure after a successful commit', async () => {
        const httpPost = jest
            .fn()
            .mockResolvedValueOnce({ status: 201, data: { accepted: true } })
            .mockResolvedValueOnce({ status: 422, data: { error: 'reveal rejected' } });

        const result = await submitCommitReveal(
            { temperature_c: 1.5, humidity_pct: 80, recorded_at: '2026-03-20T00:00:00.000Z' },
            'PWS-TEST-001',
            '0x1234',
            { httpPost, randomBytes: () => Buffer.alloc(16, 0x22), commitBlockHeight: 42 },
        );

        expect(result).toEqual({
            ok: false,
            phase: 'reveal',
            status: 422,
            detail: { error: 'reveal rejected' },
        });
        expect(httpPost).toHaveBeenCalledTimes(2);
    });

    test('pending reveals are persisted, retried, and cleared after a transient reveal failure', async () => {
        const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'wrfcoin-collector-state-'));
        const stateFilePath = path.join(tempDir, 'pending-reveals.json');
        const nonceBuffer = Buffer.from('00112233445566778899aabbccddeeff', 'hex');
        const reading = {
            humidity_pct: 61,
            recorded_at: '2026-03-20T01:02:03.000Z',
            temperature_c: 4.2,
        };
        const stationId = 'PWS-TEST-001';
        const contributor = '0x1234';
        const expectedPayload = sortKeys(reading);
        const expectedTimestamp = Math.floor(new Date(reading.recorded_at).getTime() / 1000);
        const expectedHash = computeCommitId(
            expectedPayload,
            nonceBuffer.toString('hex'),
            stationId,
            expectedTimestamp,
            'contribution',
        );
        const httpPost = jest
            .fn()
            .mockResolvedValueOnce({ status: 201, data: { accepted: true } })
            .mockResolvedValueOnce({ status: 503, data: { error: 'reveal unavailable' } })
            .mockResolvedValueOnce({ status: 201, data: { revealed: true } });

        try {
            const failedSubmit = await submitCommitReveal(reading, stationId, contributor, {
                host: '127.0.0.1',
                port: 8556,
                token: 'operator-token',
                httpPost,
                randomBytes: () => nonceBuffer,
                stateFilePath,
                commitBlockHeight: 91,
            });

            expect(failedSubmit).toEqual({
                ok: false,
                phase: 'reveal',
                status: 503,
                detail: { error: 'reveal unavailable' },
            });

            const persisted = JSON.parse(fs.readFileSync(stateFilePath, 'utf8'));
            expect(persisted).toHaveLength(1);
            expect(persisted[0]).toMatchObject({
                commit_hash: expectedHash,
                nonce: nonceBuffer.toString('hex'),
                commit_block_height: 91,
                station_id: stationId,
                contributor,
                reading: expectedPayload,
            });

            const recovery = await processPendingReveals({
                host: '127.0.0.1',
                port: 8556,
                token: 'operator-token',
                httpPost,
                stateFilePath,
                currentBlockHeight: 92,
            });

            expect(recovery).toMatchObject({
                attempted: 1,
                revealed: 1,
                failed: 0,
                dropped: 0,
            });

            const cleared = JSON.parse(fs.readFileSync(stateFilePath, 'utf8'));
            expect(cleared).toEqual([]);
            expect(httpPost).toHaveBeenNthCalledWith(
                3,
                '127.0.0.1',
                8556,
                '/transactions/contribution/reveal',
                {
                    contributor,
                    station_id: stationId,
                    payload: expectedPayload,
                    nonce: nonceBuffer.toString('hex'),
                    commit_hash: expectedHash,
                    timestamp: expectedTimestamp,
                },
                'operator-token',
            );
        } finally {
            fs.rmSync(tempDir, { recursive: true, force: true });
        }
    });

    test('two retries of the same reading keep only one pending reveal entry', async () => {
        const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'wrfcoin-collector-state-'));
        const stateFilePath = path.join(tempDir, 'pending-reveals.json');
        const reading = {
            humidity_pct: 61,
            recorded_at: '2026-03-20T01:02:03.000Z',
            temperature_c: 4.2,
        };
        const stationId = 'PWS-TEST-001';
        const contributor = '0x1234';
        const firstNonce = Buffer.from('00112233445566778899aabbccddeeff', 'hex');
        const secondNonce = Buffer.from('ffeeddccbbaa99887766554433221100', 'hex');
        const expectedPayload = sortKeys(reading);
        const expectedTimestamp = Math.floor(new Date(reading.recorded_at).getTime() / 1000);
        const firstCommitHash = computeCommitId(
            expectedPayload,
            firstNonce.toString('hex'),
            stationId,
            expectedTimestamp,
            'contribution',
        );
        const secondCommitHash = computeCommitId(
            expectedPayload,
            secondNonce.toString('hex'),
            stationId,
            expectedTimestamp,
            'contribution',
        );
        const expectedReadingId = computePendingRevealReadingId(reading, stationId, expectedTimestamp);
        const httpPost = jest
            .fn()
            .mockResolvedValueOnce({ status: 201, data: { accepted: true } })
            .mockResolvedValueOnce({ status: 503, data: { error: 'reveal unavailable' } })
            .mockResolvedValueOnce({ status: 201, data: { accepted: true } })
            .mockResolvedValueOnce({ status: 503, data: { error: 'reveal unavailable' } });

        try {
            await submitCommitReveal(reading, stationId, contributor, {
                host: '127.0.0.1',
                port: 8556,
                token: 'operator-token',
                httpPost,
                randomBytes: () => firstNonce,
                stateFilePath,
                commitBlockHeight: 91,
            });

            await submitCommitReveal(reading, stationId, contributor, {
                host: '127.0.0.1',
                port: 8556,
                token: 'operator-token',
                httpPost,
                randomBytes: () => secondNonce,
                stateFilePath,
                commitBlockHeight: 92,
            });

            const persisted = JSON.parse(fs.readFileSync(stateFilePath, 'utf8'));
            expect(persisted).toHaveLength(1);
            expect(persisted[0]).toMatchObject({
                reading_id: expectedReadingId,
                commit_hash: secondCommitHash,
                nonce: secondNonce.toString('hex'),
                commit_block_height: 92,
                station_id: stationId,
                contributor,
                reading: expectedPayload,
            });
            expect(persisted[0].commit_hash).not.toBe(firstCommitHash);
            expect(httpPost).toHaveBeenCalledTimes(4);
        } finally {
            fs.rmSync(tempDir, { recursive: true, force: true });
        }
    });

    test('pending reveals older than the max age are dropped without retrying', async () => {
        const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'wrfcoin-collector-state-'));
        const stateFilePath = path.join(tempDir, 'pending-reveals.json');
        const httpPost = jest.fn();

        try {
            fs.writeFileSync(stateFilePath, JSON.stringify([
                {
                    commit_hash: 'old-commit',
                    nonce: '00112233445566778899aabbccddeeff',
                    reading: { temperature_c: 1.2, recorded_at: '2026-03-20T00:00:00.000Z' },
                    commit_block_height: 1,
                    station_id: 'PWS-TEST-001',
                    contributor: '0x1234',
                    timestamp: 1700000000,
                },
            ], null, 2));

            const recovery = await processPendingReveals({
                host: '127.0.0.1',
                port: 8556,
                token: 'operator-token',
                httpPost,
                stateFilePath,
                currentBlockHeight: 200,
            });

            expect(recovery).toMatchObject({
                attempted: 1,
                revealed: 0,
                failed: 0,
                dropped: 1,
            });
            expect(httpPost).not.toHaveBeenCalled();

            const cleared = JSON.parse(fs.readFileSync(stateFilePath, 'utf8'));
            expect(cleared).toEqual([]);
        } finally {
            fs.rmSync(tempDir, { recursive: true, force: true });
        }
    });

    test('checkpoint timestamp never regresses when only recovered readings succeed', () => {
        const previousLastTs = Date.parse('2026-03-21T00:00:00.000Z');
        const olderReadings = [
            { recorded_at: '2026-03-20T01:02:03.000Z' },
            { recorded_at: '2026-03-19T04:05:06.000Z' },
        ];
        const newerReadingTs = Date.parse('2026-03-22T01:02:03.000Z');

        expect(computeNextLastSubmittedTs(previousLastTs, olderReadings)).toBe(previousLastTs);
        expect(computeNextLastSubmittedTs(previousLastTs, [...olderReadings, {
            recorded_at: '2026-03-22T01:02:03.000Z',
        }])).toBe(newerReadingTs);
    });

    test('mineBlock posts the explicitly configured miner payload', async () => {
        const httpPost = jest.fn().mockResolvedValue({ status: 201, data: { height: 99 } });
        const minerAddress = '0x680d51f7543ca70029866956376306d47ce60ed0';

        const result = await mineBlock({
            host: '127.0.0.1',
            port: 8556,
            token: 'operator-token',
            minerAddress,
            httpPost,
        });

        expect(result).toEqual({ status: 201, data: { height: 99 } });
        expect(httpPost).toHaveBeenCalledWith(
            '127.0.0.1',
            8556,
            '/blocks/mine',
            { miner_address: minerAddress },
            'operator-token',
        );
    });

});
