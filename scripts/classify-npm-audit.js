#!/usr/bin/env node

// Turn two `npm audit --json` reports into a reviewable classification.
// The second report must be produced with `--omit=dev`; current npm v2
// reports do not expose a reliable per-vulnerability `dev` field.
// This script is read-only and never invokes `npm audit fix`.
import fs from 'node:fs';

const [fullPath, productionPath, lockPath = 'package-lock.json'] = process.argv.slice(2);
if (!fullPath || !productionPath) {
  console.error(
    'usage: node scripts/classify-npm-audit.js <full-audit.json> <omit-dev-audit.json> [package-lock.json]',
  );
  process.exit(2);
}

function readAudit(path, label) {
  const report = JSON.parse(fs.readFileSync(path, 'utf8'));
  if (report.error || report.auditReportVersion !== 2 || !report.vulnerabilities) {
    throw new Error(`${label} is not a successful npm audit v2 report`);
  }
  const rowCount = Object.keys(report.vulnerabilities).length;
  if (report.metadata?.vulnerabilities?.total !== rowCount) {
    throw new Error(
      `${label} metadata total ${report.metadata?.vulnerabilities?.total} does not match ${rowCount} vulnerability records`,
    );
  }
  return report;
}

const full = readAudit(fullPath, 'full audit');
const production = readAudit(productionPath, 'production audit');
const lock = JSON.parse(fs.readFileSync(lockPath, 'utf8'));
const declarations = {
  ...(lock.packages?.['']?.dependencies || {}),
  ...(lock.packages?.['']?.devDependencies || {}),
};
const productionNames = new Set(Object.keys(production.vulnerabilities));

for (const name of productionNames) {
  if (!full.vulnerabilities[name]) {
    throw new Error(`production audit contains ${name}, which is absent from the full audit`);
  }
}

function classifyFix(fix) {
  if (fix === true) return { kind: 'non-major', target: 'automatic within current range' };
  if (!fix) return { kind: 'none', target: 'none available' };
  return {
    kind: fix.isSemVerMajor ? 'major' : 'non-major',
    target: `${fix.name}@${fix.version}`,
  };
}

const SURFACE_CLASSIFICATION = {
  '@chainlink/contracts': {
    surface: 'Smart-contract interface package',
    exposure: 'Low-to-moderate (ABI compatibility)',
    importers: 'legacy EVM toolchain path',
    note: 'Provides chainlink contract types consumed by contract/deps integration layers.',
  },
  '@redocly/cli': {
    surface: 'OpenAPI documentation CLI',
    exposure: 'Build-tool only',
    importers: '0 runtime source files',
    note: 'Used in documentation generation and lint workflows.',
  },
  artillery: {
    surface: 'Load-test tooling',
    exposure: 'Dev/build only',
    importers: '0 runtime source files',
    note: 'Synthetic load and benchmark tooling for non-production pipelines.',
  },
  bip32: {
    surface: 'HD key derivation (wallet key handling)',
    exposure: 'High sensitivity, isolated',
    importers: '1 file in deprecated SDK',
    note: 'Crypto key derivation path is present in deprecated JS SDK; core node crypto uses Rust implementations.',
  },
  'body-parser': {
    surface: 'Express request body parsing middleware',
    exposure: 'Moderate ingress exposure',
    importers: '0 direct source files in core app',
    note: 'Parses HTTP body payloads for legacy HTTP stacks.',
  },
  dompurify: {
    surface: 'DOM sanitization',
    exposure: 'High sensitivity, isolated',
    importers: '0 direct source files',
    note: 'Sanitizes HTML/DOM payloads to reduce XSS risk.',
  },
  ethers: {
    surface: 'EVM/web3 client utilities',
    exposure: 'Moderate, legacy/SDK use',
    importers: '5 files (legacy SDK + tests)',
    note: 'Contract and wallet utilities for legacy bridge SDK and test harnesses.',
  },
  'js-yaml': {
    surface: 'YAML parsing/serialization',
    exposure: 'Low-to-moderate',
    importers: '2 files (docs + integration tests)',
    note: 'Used for doc/config parsing in tooling paths.',
  },
  'node-vault': {
    surface: 'Vault client for secret retrieval',
    exposure: 'High sensitivity',
    importers: '0 direct source files',
    note: 'Secret material fetch client; production callers are env-based today.',
  },
  'react-router-dom': {
    surface: 'Client-side routing',
    exposure: 'Frontend/browser surface',
    importers: 'legacy front-end paths',
    note: 'Route mapping and browser-history handling in UI-facing code.',
  },
  turbo: {
    surface: 'Monorepo task orchestrator',
    exposure: 'Wide build-tool blast radius',
    importers: '0 source files',
    note: 'CLI runner for repo-wide build/test/lint graph execution.',
  },
  'ua-parser-js': {
    surface: 'User-Agent parsing',
    exposure: 'Low',
    importers: '0 direct source files',
    note: 'Parses UA headers for telemetry and compatibility metadata.',
  },
  vite: {
    surface: 'Frontend bundler and dev server',
    exposure: 'Build/dev only',
    importers: '0 source files',
    note: 'Modern frontend toolchain used outside core consensus runtime.',
  },
};

function surfaceReviewFor(name) {
  return (
    SURFACE_CLASSIFICATION[name] || {
      surface: 'No explicit surface note found in this classification pack',
      exposure: 'unknown',
      importers: 'unknown',
      note: 'No functional-surface review text has been registered for this package.',
    }
  );
}

const rows = Object.entries(full.vulnerabilities).map(([name, item]) => {
  const fix = classifyFix(item.fixAvailable);
  const surface = surfaceReviewFor(name);
  return {
    name,
    severity: item.severity || 'unknown',
    production: productionNames.has(name),
    direct: Boolean(item.isDirect),
    fix: fix.kind,
    target: fix.target,
    declared: declarations[name] || '',
    locked: lock.packages?.[`node_modules/${name}`]?.version || '',
    surface: surface.surface,
    exposure: surface.exposure,
    importers: surface.importers,
    surfaceNote: surface.note,
    via: (item.via || [])
      .map((entry) => (typeof entry === 'string' ? entry : entry.source))
      .filter(Boolean),
  };
});

const count = (predicate) => rows.filter(predicate).length;
const advisoryIds = new Set(
  Object.values(full.vulnerabilities)
    .flatMap((item) => item.via || [])
    .filter((entry) => typeof entry === 'object' && entry.source)
    .map((entry) => entry.source),
);
const escapeCell = (value) => String(value).replaceAll('|', '\\|').replaceAll('\n', ' ');

console.log('# npm audit classification');
console.log('');
console.log(`- Vulnerable package records: ${rows.length}`);
console.log(`- Unique advisory IDs referenced: ${advisoryIds.size}`);
console.log(`- Production-reachable: ${count((row) => row.production)}`);
console.log(`- Dev/build-only: ${count((row) => !row.production)}`);
console.log(`- Direct: ${count((row) => row.direct)}`);
console.log(`- Transitive: ${count((row) => !row.direct)}`);
console.log(`- Fixable without a major path: ${count((row) => row.fix === 'non-major')}`);
console.log(`- Major-path decision required: ${count((row) => row.fix === 'major')}`);
console.log(`- No fix available: ${count((row) => row.fix === 'none')}`);
console.log(`- Direct, non-major candidates: ${count((row) => row.direct && row.fix === 'non-major')}`);
console.log('');
console.log('## Reachability by directness');
console.log('');
console.log('| Reachability | Direct | Transitive | Total |');
console.log('|---|---:|---:|---:|');
for (const [label, predicate] of [
  ['Production', (row) => row.production],
  ['Dev/build only', (row) => !row.production],
]) {
  console.log(
    `| ${label} | ${count((row) => predicate(row) && row.direct)} | ${count((row) => predicate(row) && !row.direct)} | ${count(predicate)} |`,
  );
}
console.log('');
console.log('## Reachability by remediation path');
console.log('');
console.log('| Reachability | Non-major | Major | No fix | Total |');
console.log('|---|---:|---:|---:|---:|');
for (const [label, predicate] of [
  ['Production', (row) => row.production],
  ['Dev/build only', (row) => !row.production],
]) {
  console.log(
    `| ${label} | ${count((row) => predicate(row) && row.fix === 'non-major')} | ${count((row) => predicate(row) && row.fix === 'major')} | ${count((row) => predicate(row) && row.fix === 'none')} | ${count(predicate)} |`,
  );
}
console.log('');
console.log('## Direct, non-major candidates');
console.log('');
console.log('| Package | Severity | Reachability | Functional Surface | Exposure | Importers | Declared | Locked | Audit fix path | Surface note |');
console.log('|---|---|---|---|---|---|---|---|---|---|');
for (const row of rows
  .filter((candidate) => candidate.direct && candidate.fix === 'non-major')
  .sort((a, b) => a.name.localeCompare(b.name))) {
  console.log(
    `| ${escapeCell(row.name)} | ${escapeCell(row.severity)} | ${row.production ? 'production' : 'dev/build'} | ${escapeCell(row.surface)} | ${escapeCell(row.exposure)} | ${escapeCell(row.importers)} | ${escapeCell(row.declared)} | ${escapeCell(row.locked)} | ${escapeCell(row.target)} | ${escapeCell(row.surfaceNote)} |`,
  );
}
console.log('');
console.log('## Complete vulnerable-package inventory');
console.log('');
console.log('| Package | Severity | Reachability | Directness | Locked | Fix path | Target | Via |');
console.log('|---|---|---|---|---|---|---|---|');
for (const row of rows.sort((a, b) => a.name.localeCompare(b.name))) {
  console.log(
    `| ${escapeCell(row.name)} | ${escapeCell(row.severity)} | ${row.production ? 'production' : 'dev/build'} | ${row.direct ? 'direct' : 'transitive'} | ${escapeCell(row.locked)} | ${row.fix} | ${escapeCell(row.target)} | ${escapeCell(row.via.join(', '))} |`,
  );
}
