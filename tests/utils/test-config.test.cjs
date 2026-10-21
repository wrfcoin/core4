'use strict';

const assert = require('node:assert/strict');
const test = require('node:test');

const {
    buildUsage,
    parseCliArguments,
    resolveOptionValues,
} = require('./test-config.cjs');

const DEFINITIONS = [
    { name: 'help', type: 'boolean' },
    { name: 'host', type: 'string', env: 'WRFCOIN_API_HOST', default: '127.0.0.1' },
    { name: 'port', type: 'int', env: 'WRFCOIN_API_PORT', default: 8556 },
    { name: 'token', type: 'string', env: 'WRFCOIN_OPERATOR_TOKEN' },
    { name: 'count', type: 'int', default: 100 },
    { name: 'dry-run', type: 'boolean' },
    { name: 'replay-file', type: 'string' },
];

test('parses spaced arguments', () => {
    const parsed = parseCliArguments(['--count', '5', '--host', '10.0.0.8', '--dry-run'], DEFINITIONS);
    assert.equal(parsed.errors.length, 0);
    assert.equal(parsed.values.count, 5);
    assert.equal(parsed.values.host, '10.0.0.8');
    assert.equal(parsed.values['dry-run'], true);
});

test('parses equals arguments', () => {
    const parsed = parseCliArguments(['--count=7', '--replay-file=fixture.json'], DEFINITIONS);
    assert.equal(parsed.errors.length, 0);
    assert.equal(parsed.values.count, 7);
    assert.equal(parsed.values['replay-file'], 'fixture.json');
});

test('rejects invalid integers', () => {
    const parsed = parseCliArguments(['--count', 'abc'], DEFINITIONS);
    assert.equal(parsed.errors.length, 1);
    assert.match(parsed.errors[0], /expects an integer/);
});

test('applies env fallbacks and defaults', () => {
    const parsed = parseCliArguments([], DEFINITIONS);
    const resolved = resolveOptionValues(parsed, {
        WRFCOIN_API_HOST: 'api.internal',
        WRFCOIN_OPERATOR_TOKEN: 'abcd'.repeat(16),
    });

    assert.equal(resolved.values.host, 'api.internal');
    assert.equal(resolved.values.port, 8556);
    assert.equal(resolved.values.count, 100);
    assert.equal(resolved.values.token, 'abcd'.repeat(16));
    assert.equal(resolved.sources.host, 'env:WRFCOIN_API_HOST');
    assert.equal(resolved.sources.port, 'default');
    assert.equal(resolved.sources.count, 'default');
});

test('usage text includes env and default metadata', () => {
    const usage = buildUsage('tests/stress/test-burst-transfers.cjs', 'Burst transfer runner', DEFINITIONS);
    assert.match(usage, /Usage: node tests\/stress\/test-burst-transfers\.cjs \[options\]/);
    assert.match(usage, /env: WRFCOIN_API_HOST/);
    assert.match(usage, /default: 8556/);
});
