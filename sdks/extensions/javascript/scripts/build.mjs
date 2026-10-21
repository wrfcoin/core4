import { copyFile, rm } from 'node:fs/promises';
import { spawnSync } from 'node:child_process';
import { resolve } from 'node:path';

const packageRoot = resolve(import.meta.dirname, '..');
await rm(resolve(packageRoot, 'dist'), { recursive: true, force: true });

const result = spawnSync(
  process.execPath,
  [resolve(packageRoot, 'node_modules', 'typescript', 'bin', 'tsc')],
  { cwd: packageRoot, stdio: 'inherit' }
);
if (result.status !== 0) {
  process.exit(result.status ?? 1);
}

const curatedDeclarations = [
  'AirQualityMonitorClient.d.ts',
  'PhysicalDeviceAdapter.d.ts',
  'WeatherStationClient.d.ts',
];
await Promise.all(
  curatedDeclarations.map(async file => {
    await copyFile(
      resolve(packageRoot, 'iot-devices', file),
      resolve(packageRoot, 'dist', 'iot-devices', file)
    );
    await rm(resolve(packageRoot, 'dist', 'iot-devices', `${file}.map`), {
      force: true,
    });
  })
);
