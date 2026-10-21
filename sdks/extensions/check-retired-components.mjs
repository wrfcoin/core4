import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { dirname, isAbsolute, normalize, relative, resolve, sep } from 'node:path';
import { spawnSync } from 'node:child_process';

const repoRoot = resolve(import.meta.dirname, '..', '..');
const retiredNames = [
  '@wrfcoin/sdk-react',
  '@wrfcoin/sdk-vue',
  '@wrfcoin/react-components',
  '@wrfcoin/vue-components',
];
const retiredPaths = [
  'sdks/extensions/components/react',
  'sdks/extensions/components/vue',
];
const retiredDownloadClaims = [
  /\/api\/developer\/sdk\/download\/react(?:\b|\/)/i,
  /\/api\/developer\/sdk\/download\/vue(?:\b|\/)/i,
];
const retiredFrameworkClaims = [
  /React\s+hooks\s+and\s+components/i,
  /Vue(?:\.js)?\s+composables/i,
  /(?:ready-to-use|pre-built|available|supported)[^\n]{0,80}\bReact\b[^\n]{0,40}\bVue(?:\.js)?\b[^\n]{0,80}\b(?:components?|hooks?|composables?)\b/i,
];

function stringValues(value) {
  if (typeof value === 'string') return [value];
  if (Array.isArray(value)) return value.flatMap(stringValues);
  if (value && typeof value === 'object') {
    return Object.values(value).flatMap(stringValues);
  }
  return [];
}

function resolvesIntoRetiredTree(manifestPath, value) {
  const withoutFileScheme = value.startsWith('file:') ? value.slice(5) : value;
  const resolved = normalize(
    resolve(repoRoot, dirname(manifestPath), withoutFileScheme)
  );
  return retiredPaths.some(retiredPath => {
    const retiredRoot = resolve(repoRoot, retiredPath);
    const fromRetiredRoot = relative(retiredRoot, resolved);
    return (
      fromRetiredRoot === '' ||
      (!fromRetiredRoot.startsWith(`..${sep}`) &&
        fromRetiredRoot !== '..' &&
        !isAbsolute(fromRetiredRoot))
    );
  });
}

function manifestViolations(path, manifest) {
  const serialized = JSON.stringify(manifest);
  const violations = [];
  for (const retiredName of retiredNames) {
    if (serialized.includes(retiredName)) {
      violations.push(`${path}: manifest references retired package ${retiredName}`);
    }
  }
  for (const retiredPath of retiredPaths) {
    if (serialized.includes(retiredPath)) {
      violations.push(`${path}: exports or packages ${retiredPath}`);
    }
  }
  for (const value of stringValues(manifest)) {
    if (resolvesIntoRetiredTree(path, value)) {
      violations.push(`${path}: relative export resolves into a retired component tree`);
    }
  }
  return violations;
}

assert.equal(
  manifestViolations('negative/package.json', {
    name: '@wrfcoin/sdk-react',
    exports: './sdks/extensions/components/react/index.js',
  }).length,
  2,
  'negative control must detect a restored package/export claim'
);
assert.equal(
  manifestViolations('consumer/package.json', {
    name: 'consumer',
    dependencies: { '@wrfcoin/react-components': '^1.0.0' },
  }).length,
  1,
  'negative control must detect retired packages in dependency keys'
);
assert.equal(
  manifestViolations('sdks/extensions/package.json', {
    name: '@wrfcoin/sdk-extensions',
    exports: './components/react/components/index.js',
  }).length,
  1,
  'negative control must resolve exports relative to their manifest'
);
assert.equal(
  manifestViolations('sdks/extensions/package.json', {
    name: '@wrfcoin/sdk-extensions',
    files: ['components/react'],
  }).length,
  1,
  'negative control must resolve bare manifest paths into retired trees'
);
assert.ok(
  retiredDownloadClaims.some(pattern =>
    pattern.test('/api/developer/sdk/download/react')
  ),
  'negative control must detect a restored retired-SDK download claim'
);
assert.ok(
  retiredFrameworkClaims.some(pattern =>
    pattern.test('React hooks and components')
  ),
  'negative control must detect a generic retired framework claim'
);
assert.ok(
  retiredFrameworkClaims.some(pattern =>
    pattern.test('UI Components: Ready-to-use React and Vue.js components')
  ),
  'negative control must detect ready-to-use React and Vue component claims'
);

const listed = spawnSync('git', ['ls-files', '-z'], {
  cwd: repoRoot,
  encoding: 'utf8',
});
assert.equal(listed.status, 0, listed.stderr);
const tracked = listed.stdout.split('\0').filter(Boolean);
const violations = [];
let manifestsChecked = 0;

for (const path of tracked.filter(entry => entry.endsWith('package.json'))) {
  const manifest = JSON.parse(await readFile(resolve(repoRoot, path), 'utf8'));
  manifestsChecked += 1;
  violations.push(...manifestViolations(path, manifest));
}

for (const path of [
  'sdks/extensions/components/react/package.json',
  'sdks/extensions/components/vue/package.json',
]) {
  assert.ok(!tracked.includes(path), `${path} must not exist`);
}

for (const path of tracked.filter(
  entry =>
    entry.endsWith('.md') &&
    !entry.startsWith('sdks/deprecated/') &&
    !entry.startsWith('docs/archive/')
)) {
  const content = await readFile(resolve(repoRoot, path), 'utf8');
  for (const retiredName of retiredNames) {
    if (content.includes(retiredName)) {
      violations.push(`${path}: public claim references ${retiredName}`);
    }
  }
  for (const retiredDownloadClaim of retiredDownloadClaims) {
    if (retiredDownloadClaim.test(content)) {
      violations.push(`${path}: public download claim references a retired component SDK`);
    }
  }
  for (const retiredFrameworkClaim of retiredFrameworkClaims) {
    if (retiredFrameworkClaim.test(content)) {
      violations.push(`${path}: public claim advertises a retired framework integration`);
    }
  }
}

assert.deepEqual(violations, [], violations.join('\n'));
console.log(
  `retired component guard PASS: ${manifestsChecked} manifests checked; negative control RED`
);
