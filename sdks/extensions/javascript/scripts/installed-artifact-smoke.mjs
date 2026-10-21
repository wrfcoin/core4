import assert from 'node:assert/strict';
import { mkdtemp, readFile, rm, unlink, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const packageRoot = resolve(import.meta.dirname, '..');
const fixtureRoot = await mkdtemp(join(tmpdir(), 'wrfcoin-sdk-artifact-'));

function run(command, args, cwd) {
  const result = spawnSync(command, args, {
    cwd,
    encoding: 'utf8',
    env: { ...process.env, npm_config_cache: join(fixtureRoot, '.npm-cache') },
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  if (result.status !== 0) {
    throw new Error(
      `${command} ${args.join(' ')} failed (${result.status})\n${result.stdout}\n${result.stderr}`
    );
  }
  return result.stdout;
}

try {
  const packOutput = run('npm', ['pack', '--json'], packageRoot);
  const packed = JSON.parse(packOutput);
  assert.equal(packed.length, 1, 'npm pack must produce exactly one artifact');
  const tarball = join(packageRoot, packed[0].filename);

  await writeFile(
    join(fixtureRoot, 'package.json'),
    JSON.stringify({
      name: 'sdk-artifact-fixture',
      private: true,
      type: 'module',
    })
  );
  run(
    'npm',
    ['install', '--ignore-scripts', '--no-audit', '--no-fund', tarball],
    fixtureRoot
  );

  const smoke = `
    const root = await import('@wrfcoin/sdk-extensions');
    const compute = await import('@wrfcoin/sdk-extensions/compute-receipt');
    const names = [...Object.keys(root), ...Object.keys(compute)];
    if (names.length === 0) throw new Error('VOID: zero exported modules exercised');
    for (const required of ['WeatherService', 'WeatherStationClient', 'AirQualityMonitorClient', 'IoTDeviceManager']) {
      if (!(required in root)) throw new Error('missing root export: ' + required);
    }
    if (!('signComputeReceipt' in compute)) throw new Error('missing compute-receipt export');
    console.log(JSON.stringify({ exercised: names.length, required: 5 }));
  `;
  const positive = run(
    'node',
    ['--input-type=module', '--eval', smoke],
    fixtureRoot
  );
  const evidence = JSON.parse(positive.trim());
  assert.ok(evidence.exercised >= evidence.required);

  const installedRoot = join(
    fixtureRoot,
    'node_modules',
    '@wrfcoin',
    'sdk-extensions'
  );
  const manifest = JSON.parse(
    await readFile(join(installedRoot, 'package.json'))
  );
  assert.equal(manifest.exports['.'].require, undefined);

  await writeFile(
    join(fixtureRoot, 'consumer.ts'),
    `import {
      AirQualityMonitorClient,
      IoTDeviceManager,
      type PhysicalDeviceAdapter,
      WeatherStationClient,
    } from '@wrfcoin/sdk-extensions';

    declare const sdk: {
      registerIoTDevice(data: unknown): Promise<{ id: string }>;
      submitDeviceData(data: unknown): Promise<unknown>;
      updateDeviceStatus(data: unknown): Promise<unknown>;
      updateDeviceInfo(data: unknown): Promise<unknown>;
      submitAlert(data: unknown): Promise<unknown>;
    };
    const weatherAdapter = {
      readWeatherData: async () => ({}),
    } satisfies PhysicalDeviceAdapter;
    const airAdapter = {
      readAirQualityData: async () => ({}),
    } satisfies PhysicalDeviceAdapter;
    new IoTDeviceManager({ sdk });
    new WeatherStationClient({ sdk, adapter: weatherAdapter });
    new AirQualityMonitorClient({
      sdk,
      adapter: airAdapter,
      maxQueueSize: 25,
      autoRetry: false,
      retryDelay: 1_000,
    });
    `
  );
  await writeFile(
    join(fixtureRoot, 'tsconfig.json'),
    JSON.stringify({
      compilerOptions: {
        target: 'ES2022',
        module: 'NodeNext',
        moduleResolution: 'NodeNext',
        strict: true,
        noEmit: true,
        skipLibCheck: false,
      },
      files: ['consumer.ts'],
    })
  );
  run(
    join(packageRoot, 'node_modules', '.bin', 'tsc'),
    ['--project', 'tsconfig.json'],
    fixtureRoot
  );

  await writeFile(
    join(fixtureRoot, 'missing-adapter.ts'),
    `import { WeatherStationClient } from '@wrfcoin/sdk-extensions';
    declare const sdk: {
      registerIoTDevice(data: unknown): Promise<{ id: string }>;
      submitDeviceData(data: unknown): Promise<unknown>;
      updateDeviceInfo(data: unknown): Promise<unknown>;
    };
    new WeatherStationClient({ sdk });
    `
  );
  await writeFile(
    join(fixtureRoot, 'missing-adapter-tsconfig.json'),
    JSON.stringify({
      compilerOptions: {
        target: 'ES2022',
        module: 'NodeNext',
        moduleResolution: 'NodeNext',
        strict: true,
        noEmit: true,
        skipLibCheck: false,
      },
      files: ['missing-adapter.ts'],
    })
  );
  const missingAdapter = spawnSync(
    join(packageRoot, 'node_modules', '.bin', 'tsc'),
    ['--project', 'missing-adapter-tsconfig.json'],
    { cwd: fixtureRoot, encoding: 'utf8' }
  );
  assert.notEqual(
    missingAdapter.status,
    0,
    'negative fixture must reject a client constructed without an adapter'
  );
  assert.match(
    `${missingAdapter.stdout}\n${missingAdapter.stderr}`,
    /adapter/,
    'negative fixture must fail specifically on the missing adapter contract'
  );

  await unlink(
    join(installedRoot, 'dist', 'iot-devices', 'WeatherStationClient.js')
  );
  const negative = spawnSync(
    'node',
    [
      '--input-type=module',
      '--eval',
      "await import('@wrfcoin/sdk-extensions')",
    ],
    { cwd: fixtureRoot, encoding: 'utf8' }
  );
  assert.notEqual(
    negative.status,
    0,
    'negative fixture must fail when an exported implementation is removed'
  );

  console.log(
    `installed artifact PASS: ${evidence.exercised} exports exercised; strict consumer types compile; missing-adapter and missing-file controls RED`
  );
  await rm(tarball, { force: true });
} finally {
  await rm(fixtureRoot, { recursive: true, force: true });
}
