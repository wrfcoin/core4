import assert from 'node:assert/strict';
import test, { type TestContext } from 'node:test';
import { execFileSync } from 'node:child_process';
import { copyFileSync, mkdirSync, mkdtempSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { fileURLToPath, pathToFileURL } from 'node:url';

// Copy the production module so its relative dotenv path belongs exclusively to
// a synthetic fixture. Never import the checkout module or read its .env.
function configFixture(t: TestContext, dotenv?: string) {
  const directory = mkdtempSync(join(tmpdir(), 'sol-i-mcp-config-'));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  mkdirSync(join(directory, 'src'));
  copyFileSync(new URL('../config.ts', import.meta.url), join(directory, 'src/config.ts'));
  writeFileSync(join(directory, 'package.json'), '{"type":"module"}');
  symlinkSync(fileURLToPath(new URL('../../node_modules', import.meta.url)), join(directory, 'node_modules'), 'dir');
  if (dotenv !== undefined) writeFileSync(join(directory, '.env'), dotenv);
  const source = pathToFileURL(join(directory, 'src/config.ts')).href;
  const code = `import { loadConfig } from ${JSON.stringify(source)}; console.log(JSON.stringify(loadConfig().services))`;
  return (env: NodeJS.ProcessEnv) => JSON.parse(execFileSync(process.execPath, ['--import', import.meta.resolve('tsx'), '--input-type=module', '-e', code], {
    cwd: directory, env: { PATH: process.env.PATH, ...env }, encoding: 'utf8',
  }).trim());
}

test('backend families default to the verified unified entrypoint, with explicit overrides', (t) => {
  const run = configFixture(t);
  assert.deepEqual(run({}), { api: 'http://localhost:3001', wallet: 'http://localhost:3001', weather: 'http://localhost:3001', governance: 'http://localhost:3001', chain: 'http://localhost:8556' });
  assert.deepEqual(run({ WRFCOIN_API_HOST: 'http://127.0.0.1:1', WRFCOIN_WALLET_HOST: 'http://127.0.0.1:2', WRFCOIN_CHAIN_HOST: 'http://127.0.0.1:3' }), {
    api: 'http://127.0.0.1:1', wallet: 'http://127.0.0.1:2', weather: 'http://127.0.0.1:1', governance: 'http://127.0.0.1:1', chain: 'http://127.0.0.1:3',
  });
});

test('supported dotenv configuration is tested only in a synthetic fixture', (t) => {
  const run = configFixture(t, 'WRFCOIN_API_HOST=http://synthetic-fixture.invalid:3001\nWRFCOIN_WALLET_HOST=http://synthetic-wallet.invalid:3001\n');
  assert.deepEqual(run({ WRFCOIN_API_HOST: 'http://127.0.0.1:4' }), {
    api: 'http://127.0.0.1:4', wallet: 'http://synthetic-wallet.invalid:3001', weather: 'http://127.0.0.1:4', governance: 'http://127.0.0.1:4', chain: 'http://localhost:8556',
  });
});
