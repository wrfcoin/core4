'use strict';

/**
 * Seeds the local testnet with weather data contributions from personal
 * weather stations to generate non-zero trust/reward timeline entries.
 *
 * Usage:
 *   WRFCOIN_OPERATOR_TOKEN=... node tests/security/seed-weather-data.cjs
 */

const http = require('http');
const crypto = require('crypto');
const {
    formatRuntimeReport,
    loadTestConfig,
} = require('../utils/test-config.cjs');

const config = loadTestConfig({
    scriptName: 'tests/security/seed-weather-data.cjs',
    description: 'Seed weather contributions to populate trust/reward timeline data.',
    requireToken: true,
});

const API_HOST = config.apiHost;
const API_PORT = config.apiPort;
const TOKEN = config.operatorToken;

// ---------------------------------------------------------------------------
// Synthetic community weather station examples
// ---------------------------------------------------------------------------

const STATIONS = [
    {
        station_id: 'PWS-EXAMPLE-001',
        contributor: '0x0000000000000000000000000000000000000b01',
        lat: 10.1,
        lon: 20.1,
        label: "Example Station One",
    },
    {
        station_id: 'PWS-EXAMPLE-002',
        contributor: '0x0000000000000000000000000000000000000b02',
        lat: 10.2,
        lon: 20.2,
        label: "Example Station",
    },
    {
        station_id: 'PWS-EXAMPLE-003',
        contributor: '0x0000000000000000000000000000000000000b03',
        lat: 10.3,
        lon: 20.3,
        label: "Example Station Three",
    },
];

// Round to N decimals to avoid JS float drift (e.g. 0.2*3 = 0.6000000000000001)
function r(val, decimals = 1) {
    return parseFloat(val.toFixed(decimals));
}

// Synthetic cool-weather readings (slightly varied per station)
function generateReadings(stationIndex, readingIndex) {
    return {
        temperature_c: r(-8.5 + stationIndex * 0.3 + readingIndex * 0.2),
        humidity_pct: 72 + stationIndex * 2 + readingIndex,
        pressure_hpa: r(1013.2 + stationIndex * 0.5 - readingIndex * 0.1),
        wind_speed_ms: r(4.2 + stationIndex * 0.8 + readingIndex * 0.3),
        wind_direction_deg: 270 + stationIndex * 15 + readingIndex * 5,
        precipitation_mm: readingIndex === 0 ? 0 : r(0.2 * readingIndex),
        visibility_km: r(10.0 - readingIndex * 0.5),
        cloud_cover_pct: 60 + readingIndex * 5,
        source: 'personal_weather_station',
        location: 'Duluth, MN',
    };
}

// ---------------------------------------------------------------------------
// HTTP helpers
// ---------------------------------------------------------------------------

function request(method, path, body) {
    return new Promise((resolve, reject) => {
        const options = {
            hostname: API_HOST,
            port: API_PORT,
            path,
            method,
            headers: {
                Authorization: `Bearer ${TOKEN}`,
                'Content-Type': 'application/json',
            },
        };
        const req = http.request(options, (res) => {
            let raw = '';
            res.on('data', (chunk) => { raw += chunk; });
            res.on('end', () => {
                try {
                    resolve({ status: res.statusCode, data: JSON.parse(raw) });
                } catch (_) {
                    resolve({ status: res.statusCode, data: raw });
                }
            });
        });
        req.on('error', reject);
        if (body) req.write(JSON.stringify(body));
        req.end();
    });
}

// Sort object keys recursively to match Rust serde_json's BTreeMap ordering
function sortKeys(obj) {
    if (Array.isArray(obj)) return obj.map(sortKeys);
    if (obj !== null && typeof obj === 'object') {
        return Object.keys(obj).sort().reduce((acc, key) => {
            acc[key] = sortKeys(obj[key]);
            return acc;
        }, {});
    }
    return obj;
}

// Replicate shared-protocol's compute_commit_id: SHA3-256 of
// "{data_json}||{nonce}||{station_id}||{timestamp}||{contribution_type}"
// Keys must be alphabetically sorted to match Rust serde_json (BTreeMap).
function computeCommitId(data, nonce, stationId, timestamp, contributionType) {
    const dataStr = JSON.stringify(sortKeys(data));
    const preimage = `${dataStr}||${nonce}||${stationId}||${timestamp}||${contributionType}`;
    return crypto.createHash('sha3-256').update(preimage).digest('hex');
}

// ---------------------------------------------------------------------------
// Seeding logic
// ---------------------------------------------------------------------------

async function seedContributions() {
    const READINGS_PER_STATION = 4;
    let committed = 0;
    let revealed = 0;
    let mined = 0;

    for (let r = 0; r < READINGS_PER_STATION; r++) {
        // Submit commits for all stations
        const pendingReveals = [];

        for (const station of STATIONS) {
            const payload = generateReadings(STATIONS.indexOf(station), r);
            const nonce = crypto.randomBytes(16).toString('hex');
            const timestamp = Math.floor(Date.now() / 1000) - (READINGS_PER_STATION - r) * 600;
            const commitHash = computeCommitId(payload, nonce, station.station_id, timestamp, 'contribution');

            // Submit commit
            const commitRes = await request('POST', '/transactions/contribution/commit', {
                contributor: station.contributor,
                station_id: station.station_id,
                commit_hash: commitHash,
            });

            if (commitRes.status === 201 || commitRes.status === 200) {
                committed++;
                console.log(`  COMMIT OK: ${station.label} reading #${r + 1}`);
                pendingReveals.push({ station, payload, nonce, timestamp, commitHash });
            } else {
                console.error(`  COMMIT FAIL (${commitRes.status}): ${station.label} — ${JSON.stringify(commitRes.data)}`);
            }
        }

        // Mine a block to anchor the commits
        const mineRes1 = await request('POST', '/blocks/mine', {
            miner_address: '0x0000000000000000000000000000000000000001',
        });
        if (mineRes1.status === 201) {
            mined++;
            console.log(`  MINED block ${mineRes1.data.height} (anchoring commits)`);
        }

        // Submit reveals for this round — payload keys must be sorted to match
        // serde_json BTreeMap ordering used by compute_commit_id on the server.
        for (const { station, payload, nonce, timestamp, commitHash } of pendingReveals) {
            const revealRes = await request('POST', '/transactions/contribution/reveal', {
                contributor: station.contributor,
                station_id: station.station_id,
                payload: sortKeys(payload),
                nonce,
                commit_hash: commitHash,
                timestamp,
            });

            if (revealRes.status === 201 || revealRes.status === 200) {
                revealed++;
                console.log(`  REVEAL OK: ${station.label} reading #${r + 1}`);
            } else {
                console.error(`  REVEAL FAIL (${revealRes.status}): ${station.label} — ${JSON.stringify(revealRes.data)}`);
            }
        }

        // Mine another block to process the reveals through the reward pipeline
        const mineRes2 = await request('POST', '/blocks/mine', {
            miner_address: '0x0000000000000000000000000000000000000001',
        });
        if (mineRes2.status === 201) {
            mined++;
            console.log(`  MINED block ${mineRes2.data.height} (processing reveals)\n`);
        }
    }

    console.log(`\nSeeding complete: ${committed} commits, ${revealed} reveals, ${mined} blocks mined`);

    // Check timeline
    const timeline = await request('GET', '/transactions/rewards/trust/timeline?offset=0&limit=500');
    if (timeline.status === 200) {
        const total = timeline.data.total;
        const nonZeroMinted = timeline.data.items.filter(i => i.minted_count > 0).length;
        console.log(`Timeline: ${total} entries, ${nonZeroMinted} with non-zero minted_count`);
    }
}

async function main() {
    console.log('=== Seeding Duluth PWS weather data ===\n');
    console.log(formatRuntimeReport(config, { stations: STATIONS.length, readings_per_station: 4 }));

    // Verify server is up
    const health = await request('GET', '/health');
    if (health.status !== 200) {
        console.error(`Server not ready (HTTP ${health.status})`);
        process.exit(1);
    }

    await seedContributions();
}

main().catch((err) => {
    console.error('Seed error:', err);
    process.exit(1);
});
