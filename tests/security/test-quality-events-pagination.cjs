'use strict';

/**
 * TDD-3C.1 – TDD-3C.4: Quality-events pagination consistency
 *
 * Verifies that the /transactions/quality/events endpoint returns stable,
 * non-duplicated, non-gapped results across all page-size combinations and
 * that paginated counts agree with the aggregate /summary endpoint.
 *
 * Exit 0 = all checks PASSED
 * Exit 1 = one or more checks FAILED
 *
 * Required env:
 *   WRFCOIN_OPERATOR_TOKEN  – 64-char lowercase hex operator token
 *
 * Optional env:
 *   WRFCOIN_API_HOST        – default: 127.0.0.1
 *   WRFCOIN_API_PORT        – default: 8556
 */

const http = require('http');
const {
    formatRuntimeReport,
    loadTestConfig,
} = require('../utils/test-config.cjs');

const config = loadTestConfig({
    scriptName: 'tests/security/test-quality-events-pagination.cjs',
    description: 'TDD-3C pagination consistency checks for quality events.',
    requireToken: true,
});

const API_HOST = config.apiHost;
const API_PORT = config.apiPort;
const TOKEN    = config.operatorToken;

// ---------------------------------------------------------------------------
// HTTP helpers
// ---------------------------------------------------------------------------

function getJson(path) {
    return new Promise((resolve) => {
        const options = {
            hostname: API_HOST,
            port:     API_PORT,
            path,
            method:  'GET',
            headers: { Authorization: `Bearer ${TOKEN}` },
        };

        const req = http.request(options, (res) => {
            let raw = '';
            res.on('data', (chunk) => { raw += chunk; });
            res.on('end', () => {
                try {
                    resolve({ statusCode: res.statusCode, data: JSON.parse(raw) });
                } catch (_) {
                    resolve({ statusCode: res.statusCode, data: raw });
                }
            });
        });

        req.on('error', (err) => resolve({ statusCode: 0, error: err.message }));
        req.end();
    });
}

/**
 * Walk every page of `basePath` using offset+limit pagination.
 *
 * Returns { total, items }.
 *
 * Throws if:
 *   - any page returns a non-200 status
 *   - `total` changes between pages (server-side instability)
 */
async function fetchAllPaginated(basePath, pageSize) {
    const items  = [];
    let offset   = 0;
    let total    = null;
    let page     = 0;

    while (true) {
        const sep = basePath.includes('?') ? '&' : '?';
        const url = `${basePath}${sep}offset=${offset}&limit=${pageSize}`;
        const res = await getJson(url);

        if (res.statusCode !== 200) {
            throw new Error(
                `Page ${page} (offset=${offset}) returned HTTP ${res.statusCode}: ${
                    typeof res.data === 'string' ? res.data : JSON.stringify(res.data)
                }`
            );
        }

        const body = res.data;

        if (total === null) {
            total = body.total;
        } else if (body.total !== total) {
            throw new Error(
                `Unstable total on page ${page} (offset=${offset}): ` +
                `started at ${total}, now reports ${body.total}`
            );
        }

        items.push(...body.items);
        page++;

        // Stop when we have all items or the server returned an empty page
        if (items.length >= total || body.items.length === 0) break;

        offset += pageSize;
    }

    return { total: total !== null ? total : 0, items };
}

// ---------------------------------------------------------------------------
// Individual TDD checks
// ---------------------------------------------------------------------------

/**
 * TDD-3C.1 – Paginated totals and event_ids match a single full-page fetch.
 *
 * Fetches all events in small pages (pageSize=5), then again in one request
 * (limit=500).  Verifies:
 *   a) total is stable across all small pages
 *   b) no duplicate event_ids in the paginated walk
 *   c) event_id sets are identical to the single-page fetch
 */
async function tdd3c1_paginatedTotalConsistency() {
    const BASE = '/transactions/quality/events';
    const failures = [];

    const { total: pTotal, items: pItems } = await fetchAllPaginated(BASE, 5);

    const fullRes = await getJson(`${BASE}?offset=0&limit=500`);
    if (fullRes.statusCode !== 200) {
        failures.push(`Single-page fetch returned HTTP ${fullRes.statusCode}`);
        return failures;
    }
    const full = fullRes.data;

    // (a) total agreement
    if (pTotal !== full.total) {
        failures.push(
            `total mismatch: paginated walk reports ${pTotal}, ` +
            `single-page reports ${full.total}`
        );
    } else {
        console.log(`  [3C.1] total consistent: ${pTotal} events`);
    }

    // (b) no duplicates in paginated walk
    const pIds = pItems.map((e) => e.event_id);
    const pUniq = new Set(pIds);
    if (pUniq.size !== pIds.length) {
        const seen = new Set();
        const dups = pIds.filter((id) => { const d = seen.has(id); seen.add(id); return d; });
        failures.push(
            `Duplicate event_ids in paginated walk: ${[...new Set(dups)].join(', ')}`
        );
    } else {
        console.log(`  [3C.1] no duplicate event_ids in paginated walk`);
    }

    // (c) id-sets match
    const pSorted = [...pIds].sort((a, b) => a - b);
    const fSorted = full.items.map((e) => e.event_id).sort((a, b) => a - b);
    if (JSON.stringify(pSorted) !== JSON.stringify(fSorted)) {
        failures.push(
            `event_id sets differ.\n` +
            `  paginated (${pSorted.length}): [${pSorted.slice(0,10).join(',')}${pSorted.length>10?'…':''}]\n` +
            `  single-pg (${fSorted.length}): [${fSorted.slice(0,10).join(',')}${fSorted.length>10?'…':''}]`
        );
    } else {
        console.log(`  [3C.1] event_id sets match between paginated and single-page fetch`);
    }

    return failures;
}

/**
 * TDD-3C.2 – band=low filter count matches summary.band_counters.low.
 *
 * Walks all band=low events in small pages (pageSize=3), then checks:
 *   a) total == summary.band_counters.low
 *   b) every returned item has quality_band == "low"
 */
async function tdd3c2_bandFilterConsistency() {
    const failures = [];

    const sumRes = await getJson('/transactions/quality/summary');
    if (sumRes.statusCode !== 200) {
        failures.push(`Summary fetch returned HTTP ${sumRes.statusCode}`);
        return failures;
    }
    const expectedLow = Number(sumRes.data.band_counters.low);

    const { total, items } = await fetchAllPaginated(
        '/transactions/quality/events?band=low', 3
    );

    if (total !== expectedLow) {
        failures.push(
            `band=low paginated total (${total}) != summary.band_counters.low (${expectedLow})`
        );
    } else {
        console.log(`  [3C.2] band=low total (${total}) matches summary.band_counters.low`);
    }

    const wrong = items.filter((e) => e.quality_band !== 'low');
    if (wrong.length > 0) {
        failures.push(
            `${wrong.length} item(s) returned by band=low filter have wrong quality_band: ` +
            wrong.slice(0, 5).map((e) => `${e.event_id}→${e.quality_band}`).join(', ')
        );
    } else {
        console.log(`  [3C.2] all ${items.length} filtered items have quality_band=low`);
    }

    return failures;
}

/**
 * TDD-3C.3 – event_type=slashing filter count matches summary.slashing_events.
 */
async function tdd3c3_eventTypeFilterConsistency() {
    const failures = [];

    const sumRes = await getJson('/transactions/quality/summary');
    if (sumRes.statusCode !== 200) {
        failures.push(`Summary fetch returned HTTP ${sumRes.statusCode}`);
        return failures;
    }
    const expectedSlash = Number(sumRes.data.slashing_events);

    const { total, items } = await fetchAllPaginated(
        '/transactions/quality/events?event_type=slashing', 3
    );

    if (total !== expectedSlash) {
        failures.push(
            `event_type=slashing paginated total (${total}) != ` +
            `summary.slashing_events (${expectedSlash})`
        );
    } else {
        console.log(
            `  [3C.3] event_type=slashing total (${total}) matches summary.slashing_events`
        );
    }

    const wrong = items.filter((e) => e.event_type !== 'slashing');
    if (wrong.length > 0) {
        failures.push(
            `${wrong.length} item(s) returned by event_type=slashing filter have wrong type: ` +
            wrong.slice(0, 5).map((e) => `${e.event_id}→${e.event_type}`).join(', ')
        );
    } else {
        console.log(
            `  [3C.3] all ${items.length} filtered items have event_type=slashing`
        );
    }

    return failures;
}

/**
 * TDD-3C.4 – Out-of-bounds offset returns empty items with stable total.
 *
 * Fetches a page at offset=(total+1000).  Expects:
 *   - HTTP 200
 *   - items == []
 *   - total unchanged
 */
async function tdd3c4_outOfBoundsOffset() {
    const failures = [];

    const baseRes = await getJson('/transactions/quality/events?offset=0&limit=500');
    if (baseRes.statusCode !== 200) {
        failures.push(`Base fetch returned HTTP ${baseRes.statusCode}`);
        return failures;
    }
    const baseTotal = baseRes.data.total;

    const beyondOffset = baseTotal + 1000;
    const oobRes = await getJson(
        `/transactions/quality/events?offset=${beyondOffset}&limit=10`
    );

    if (oobRes.statusCode !== 200) {
        failures.push(
            `Out-of-bounds offset returned HTTP ${oobRes.statusCode} (expected 200)`
        );
        return failures;
    }

    if (oobRes.data.items.length !== 0) {
        failures.push(
            `Out-of-bounds offset returned ${oobRes.data.items.length} item(s); expected 0`
        );
    } else {
        console.log(`  [3C.4] out-of-bounds offset returns empty items array`);
    }

    if (oobRes.data.total !== baseTotal) {
        failures.push(
            `total changed with out-of-bounds offset: was ${baseTotal}, now ${oobRes.data.total}`
        );
    } else {
        console.log(`  [3C.4] total is stable (${baseTotal}) with out-of-bounds offset`);
    }

    return failures;
}

// ---------------------------------------------------------------------------
// Runner
// ---------------------------------------------------------------------------

async function run() {
    console.log(formatRuntimeReport(config));
    const checks = [
        { id: 'TDD-3C.1', label: 'Paginated total consistency', fn: tdd3c1_paginatedTotalConsistency },
        { id: 'TDD-3C.2', label: 'band=low filter consistency', fn: tdd3c2_bandFilterConsistency },
        { id: 'TDD-3C.3', label: 'event_type=slashing filter consistency', fn: tdd3c3_eventTypeFilterConsistency },
        { id: 'TDD-3C.4', label: 'Out-of-bounds offset boundary', fn: tdd3c4_outOfBoundsOffset },
    ];

    let anyFailed = false;

    for (const { id, label, fn } of checks) {
        console.log(`\n--- ${id}: ${label} ---`);
        try {
            const failures = await fn();
            if (failures.length === 0) {
                console.log(`PASS: ${id}`);
            } else {
                failures.forEach((f) => console.error(`  FAIL: ${f}`));
                console.error(`FAIL: ${id}`);
                anyFailed = true;
            }
        } catch (err) {
            console.error(`  FAIL (exception): ${err.message}`);
            console.error(`FAIL: ${id}`);
            anyFailed = true;
        }
    }

    console.log('\n========================================');
    if (anyFailed) {
        console.error('Phase 3C Pagination Test: FAILED');
        process.exit(1);
    } else {
        console.log('Phase 3C Pagination Test: PASSED');
        process.exit(0);
    }
}

run().catch((err) => {
    console.error('Unexpected top-level error:', err);
    process.exit(1);
});
