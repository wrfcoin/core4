#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import process from 'node:process';

const args = process.argv.slice(2);
const rootIndex = args.indexOf('--root');
const root = path.resolve(rootIndex >= 0 ? args[rootIndex + 1] : '.');
const selfTest = args.includes('--self-test');
const findings = [];

function read(relative) {
  const target = path.join(root, relative);
  if (!fs.existsSync(target)) {
    findings.push(`${relative}: required release-surface evidence is missing`);
    return '';
  }
  return fs.readFileSync(target, 'utf8');
}

function reject(relative, text, patterns) {
  for (const [label, pattern] of patterns) {
    if (pattern.test(text)) findings.push(`${relative}: ${label}`);
  }
}

function walk(relative) {
  const target = path.join(root, relative);
  if (!fs.existsSync(target)) return [];
  const files = [];
  for (const entry of fs.readdirSync(target, { withFileTypes: true })) {
    const child = path.join(relative, entry.name);
    if (entry.isDirectory()) files.push(...walk(child));
    else files.push(child);
  }
  return files;
}

const routes = read('native-chain/src/bin/launch_api/routes/mod.rs');
for (const route of ['/cross_chain', '/cross_chain/status']) {
  if (!routes.includes(`.route("${route}", web::get().to(chain::cross_chain_feature_disabled))`)) {
    findings.push(`runtime: ${route} is not wired to the fail-closed handler`);
  }
}

const handler = read('native-chain/src/bin/launch_api/routes/chain.rs');
if (!/cross_chain_feature_disabled[\s\S]*?"NOT_AVAILABLE"[\s\S]*?not available in this release/.test(handler)) {
  findings.push('runtime: cross-chain handler does not explicitly return NOT_AVAILABLE');
}

const openApiFiles = [
  'docs/api/schemas/openapi.yaml',
  'docs/api-reference/openapi.yaml',
  'docs/generated/api/openapi.yaml',
  'sdks/generated/android/api/openapi.yaml',
  'sdks/generated/java/api/openapi.yaml',
];
const forbiddenRoutes = [
  ['publishes a bridge transfer/discovery path', /^\s*\/(?:api\/)?bridge(?:s)?\/(?:networks|chains|status|initiate|transfer|transfers|fees|history)[^:]*:/m],
  ['publishes a cross-chain transfer path', /^\s*\/api\/crosschain\/transfer\s*:/m],
  ['retains a bridge operation schema', /^\s{4}(?:BridgeStatus(?:Response)?|InitiateBridgeRequest|BridgeTransfer(?:Request|Response)?|CrossChainTransfer(?:StatusResponse|Request|_confirmations)?|TransferStatus|_api_crosschain_transfer_post_201_response):/m],
];
for (const relative of openApiFiles) reject(relative, read(relative), forbiddenRoutes);

const forbiddenApiSurface = [
  ['exports a generated cross-chain API', /CrossChainBridgeApi|CrossChainBridgeAPI|cross_chain_bridge_api|cross-chain-bridge-api/],
  ['exports a generated bridge operation model', /ApiCrosschainTransferPost201Response|BridgeTransfer(?:Request|Response)|CrossChainTransfer(?:Confirmations|Request)?|TransferStatus|api_crosschain_transfer_post201_response|bridge_transfer_(?:request|response)|cross_chain_transfer|transfer_status|api-crosschain-transfer-post201-response|bridge-transfer-(?:request|response)|cross-chain-transfer|transfer-status/],
  ['contains an unavailable bridge route', /\/api\/bridge(?:s)?\/|\/api\/crosschain\/transfer/],
  ['claims generated cross-chain bridge support', /Includes weather data collection, smart contracts, cross-chain bridges/],
];
for (const relative of walk('sdks/generated')) {
  if (relative.endsWith('/api/openapi.yaml')) continue;
  if (/(?:^|\/)(?:node_modules|dist|__pycache__)(?:\/|$)/.test(relative)) continue;
  reject(relative, read(relative), forbiddenApiSurface);
}

for (const relative of [
  'sdks/extensions/javascript/package.json',
  'sdks/extensions/javascript/index.ts',
  'sdks/python/wrfcoin/__init__.py',
  'sdks/python/wrfcoin/client.py',
  'sdks/python/wrfcoin/services/__init__.py',
  'sdks/python/wrfcoin/exceptions.py',
  'sdks/python/setup.py',
  'sdks/python/pyproject.toml',
  'sdks/extensions/javascript/types/index.ts',
  'sdks/extensions/javascript/errors/index.ts',
]) {
  reject(relative, read(relative), [
    ['exports an unavailable bridge provider/service', /CrossChainController|BridgeService|BridgeError|BridgeTransfer|BridgeProvider|\.\/cross-chain|"\.\/cross-chain"|\bbridge\b(?=[",])/i],
  ]);
}

for (const retiredPath of [
  'sdks/extensions/javascript/cross-chain',
  'sdks/python/wrfcoin/services/bridge.py',
  'docs/generated/api/index.html',
]) {
  const target = path.join(root, retiredPath);
  const exists = fs.existsSync(target);
  const materiallyExists = exists && (!fs.statSync(target).isDirectory() || walk(retiredPath).length > 0);
  if (materiallyExists) {
    findings.push(`${retiredPath}: retired implementation/artifact is still release-visible`);
  }
}

const publicDocs = [
  'docs/WRFCOIN_CLI_GUIDE.md',
  'docs/QUICK_REFERENCE.md',
  'docs/api/README.md',
  'docs/api/examples/code-examples.md',
  'docs/api-reference/api_reference_complete.md',
  'docs/api-reference/code-examples.md',
  'docs/api-reference/interactive-api-explorer.md',
  'docs/api/explorer/interactive-testing.md',
  'docs/developer-getting-started.md',
  'docs/examples/local-testnet-deployment.md',
  'docs/frontend-integration/cross-chain-bridges.md',
  'docs/guides/frontend-ui-binding-requirements.md',
  'docs/guides/wrf-liquidity-path.md',
  'docs/operations/bridge-operations.md',
  'docs/skills/openclaw-wrfcoin.md',
  'docs/tutorials/cross-chain-bridge.md',
  'docs/user-guides/QUICK_REFERENCE.md',
  'docs/user-guides/WRFCOIN_TERMINAL_GUIDE.md',
  'sdks/README.md',
  'sdks/extensions/README.md',
  'sdks/python/README.md',
];
const forbiddenDocClaims = [
  ['contains an unsupported bridge URL', /\/(?:api\/)?bridge(?:s)?\/(?:networks|chains|status|initiate|transfer|transfers|fees|history)/],
  ['contains an unsupported SDK call', /sdk\.bridge|CrossChainController|BridgeService/],
  ['contains an unsupported bridge CLI command', /wrfcoin-bridge|wrfcoin\s+(?:crosschain|monitor\s+bridges)/],
];
for (const relative of publicDocs) reject(relative, read(relative), forbiddenDocClaims);

if (selfTest) {
  const fixture = read('scripts/fixtures/cross-chain-release-surface/forbidden-generated-api.ts');
  const before = findings.length;
  reject('fixture', fixture, forbiddenApiSurface);
  if (findings.length === before) findings.push('self-test: forbidden generated API fixture was not detected');
  else findings.splice(before);
}

if (findings.length) {
  console.error('CROSS_CHAIN_RELEASE_SURFACE=FAIL');
  for (const finding of findings) console.error(`- ${finding}`);
  process.exit(1);
}

console.log(`CROSS_CHAIN_RELEASE_SURFACE=PASS openapi=${openApiFiles.length} public_docs=${publicDocs.length}`);
if (selfTest) console.log('MUTATION_CONTROL=PASS forbidden generated API fixture rejected');
