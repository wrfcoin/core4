'use strict';

/**
 * TDD-3D.1a – TDD-3D.1c: Trust-reward timeline pagination consistency
 *
 * Verifies that the /transactions/rewards/trust/timeline endpoint returns
 * stable, non-duplicated results across all page-size combinations and that
 * paginated counts agree with the aggregate /summary endpoint.
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
    scriptName: 'tests/security/test-trust-reward-timeline-pagination.cjs',
    description: 'TDD-3D pagination consistency checks for trust reward timeline.',
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
 * TDD-3D.1a – Paginated block_height sets match a single full-page fetch.
 *
 * Fetches all timeline entries in small pages (pageSize=4), then again in one
 * request (limit=500).  Verifies:
 *   a) total is stable across all small pages
 *   b) no duplicate block_heights in the paginated walk
 *   c) block_height sets are identical to the single-page fetch
 */
async function tdd3d1a_paginatedBlockHeightConsistency() {
    const BASE = '/transactions/rewards/trust/timeline';
    const failures = [];

    const { total: pTotal, items: pItems } = await fetchAllPaginated(BASE, 4);

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
        console.log(`  [3D.1a] total consistent: ${pTotal} timeline entries`);
    }

    // (b) no duplicates in paginated walk
    const pHeights = pItems.map((e) => e.block_height);
    const pUniq = new Set(pHeights);
    if (pUniq.size !== pHeights.length) {
        const seen = new Set();
        const dups = pHeights.filter((h) => { const d = seen.has(h); seen.add(h); return d; });
        failures.push(
            `Duplicate block_heights in paginated walk: ${[...new Set(dups)].join(', ')}`
        );
    } else {
        console.log(`  [3D.1a] no duplicate block_heights in paginated walk`);
    }

    // (c) height sets match
    const pSorted = [...pHeights].sort((a, b) => Number(a) - Number(b));
    const fSorted = full.items.map((e) => e.block_height).sort((a, b) => Number(a) - Number(b));
    if (JSON.stringify(pSorted) !== JSON.stringify(fSorted)) {
        failures.push(
            `block_height sets differ.\n` +
            `  paginated (${pSorted.length}): [${pSorted.slice(0,10).join(',')}${pSorted.length>10?'…':''}]\n` +
            `  single-pg (${fSorted.length}): [${fSorted.slice(0,10).join(',')}${fSorted.length>10?'…':''}]`
        );
    } else {
        console.log(`  [3D.1a] block_height sets match between paginated and single-page fetch`);
    }

    return failures;
}

/**
 * TDD-3D.1b – summary.minted_count == sum of items[].minted_count.
 *
 * Fetches all timeline items in a single request (limit=500), then checks:
 *   sum(items[].minted_count) == summary.minted_count
 *
 * Note: minted_count is u64 on the Rust side. Values large enough to exceed
 * Number.MAX_SAFE_INTEGER are handled via BigInt comparison.
 */
async function tdd3d1b_summaryMintedCountAgreement() {
    const failures = [];

    const timelineRes = await getJson('/transactions/rewards/trust/timeline?offset=0&limit=500');
    if (timelineRes.statusCode !== 200) {
        failures.push(`Timeline fetch returned HTTP ${timelineRes.statusCode}`);
        return failures;
    }

    const summaryRes = await getJson('/transactions/rewards/trust/summary');
    if (summaryRes.statusCode !== 200) {
        failures.push(`Summary fetch returned HTTP ${summaryRes.statusCode}`);
        return failures;
    }

    const items = timelineRes.data.items;
    const itemsSum = items.reduce((acc, item) => acc + BigInt(item.minted_count), 0n);
    const summaryCount = BigInt(summaryRes.data.minted_count);

    if (itemsSum !== summaryCount) {
        failures.push(
            `minted_count mismatch: sum of timeline items = ${itemsSum}, ` +
            `summary.minted_count = ${summaryCount}`
        );
    } else {
        console.log(
            `  [3D.1b] minted_count consistent: items sum = summary (${summaryCount})`
        );
    }

    return failures;
}

/**
 * TDD-3D.1c – Out-of-bounds offset returns empty items with stable total.
 *
 * Fetches a page at offset=(total+1000).  Expects:
 *   - HTTP 200
 *   - items == []
 *   - total unchanged
 */
async function tdd3d1c_outOfBoundsOffset() {
    const failures = [];

    const baseRes = await getJson('/transactions/rewards/trust/timeline?offset=0&limit=500');
    if (baseRes.statusCode !== 200) {
        failures.push(`Base fetch returned HTTP ${baseRes.statusCode}`);
        return failures;
    }
    const baseTotal = baseRes.data.total;

    const beyondOffset = baseTotal + 1000;
    const oobRes = await getJson(
        `/transactions/rewards/trust/timeline?offset=${beyondOffset}&limit=10`
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
        console.log(`  [3D.1c] out-of-bounds offset returns empty items array`);
    }

    if (oobRes.data.total !== baseTotal) {
        failures.push(
            `total changed with out-of-bounds offset: was ${baseTotal}, now ${oobRes.data.total}`
        );
    } else {
        console.log(`  [3D.1c] total is stable (${baseTotal}) with out-of-bounds offset`);
    }

    return failures;
}

// ---------------------------------------------------------------------------
// Runner
// ---------------------------------------------------------------------------

async function run() {
    console.log(formatRuntimeReport(config));
    const checks = [
        { id: 'TDD-3D.1a', label: 'Paginated block_height consistency', fn: tdd3d1a_paginatedBlockHeightConsistency },
        { id: 'TDD-3D.1b', label: 'summary.minted_count == items sum', fn: tdd3d1b_summaryMintedCountAgreement },
        { id: 'TDD-3D.1c', label: 'Out-of-bounds offset boundary', fn: tdd3d1c_outOfBoundsOffset },
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
        console.error('Phase 3D Pagination Test: FAILED');
        process.exit(1);
    } else {
        console.log('Phase 3D Pagination Test: PASSED');
        process.exit(0);
    }
}

run().catch((err) => {
    console.error('Unexpected top-level error:', err);
    process.exit(1);
});
