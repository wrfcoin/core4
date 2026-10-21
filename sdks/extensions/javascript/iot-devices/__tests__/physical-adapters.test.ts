import { jest } from '@jest/globals';

import { AirQualityMonitorClient } from '../AirQualityMonitorClient.js';
import { WeatherStationClient } from '../WeatherStationClient.js';

function registered<T extends object>(client: T): T {
  Object.assign(client, { registered: true, deviceId: 'device-42' });
  return client;
}

describe('physical IoT adapters', () => {
  test('production clients fail closed without a physical adapter', () => {
    const sdk = {};
    expect(() => new WeatherStationClient({ sdk } as never)).toThrow(
      'readWeatherData'
    );
    expect(() => new AirQualityMonitorClient({ sdk } as never)).toThrow(
      'readAirQualityData'
    );
  });

  test('weather adapter observations reach submitDeviceData exactly', async () => {
    const reading = {
      schema_version: 'wrf.weather.observation.v1',
      station_id: 'station-physical-7',
      observed_at_unix: 1_800_000_000,
      observation: {
        temperature_c: -3.25,
        humidity: 87,
        pressure_hpa: 998.125,
      },
    };
    const submitDeviceData = jest.fn(async () => ({ accepted: true }));
    const client = registered(
      new WeatherStationClient({
        sdk: { submitDeviceData } as never,
        adapter: { readWeatherData: async () => reading },
        autoRetry: true,
      })
    );

    await client.readAndSubmitData();

    expect(submitDeviceData).toHaveBeenCalledTimes(1);
    expect(submitDeviceData).toHaveBeenCalledWith({
      deviceId: 'device-42',
      data: reading,
      timestamp: '2027-01-15T08:00:00.000Z',
      network: 'testnet',
    });
  });

  test.each([
    ['impossible humidity', { humidity: 999999, timestamp: '2026-09-17T12:00:00Z' }],
    ['invalid timestamp', { humidity: 50, timestamp: 'not-a-time' }],
    ['non-finite temperature', { temperature: Number.NaN, timestamp: '2026-09-17T12:00:00Z' }],
    ['oversized station id', { humidity: 50, station_id: 'x'.repeat(257), timestamp: '2026-09-17T12:00:00Z' }],
  ])('rejects %s physical weather data before submission', async (_label, reading) => {
    const submitDeviceData = jest.fn();
    const client = registered(
      new WeatherStationClient({
        sdk: { submitDeviceData } as never,
        adapter: { readWeatherData: async () => reading },
      })
    );

    await expect(client.readAndSubmitData()).rejects.toThrow(/invalid|station/);
    expect(submitDeviceData).not.toHaveBeenCalled();
  });

  test('air-quality adapter observations reach submitDeviceData exactly', async () => {
    const reading = {
      pm25: 3.125,
      pm10: 7.5,
      o3: 12,
      rawFrame: '010203ff',
      timestamp: '2026-09-17T12:00:00.000Z',
    };
    const submitDeviceData = jest.fn(async () => ({ accepted: true }));
    const client = registered(
      new AirQualityMonitorClient({
        sdk: { submitDeviceData } as never,
        adapter: { readAirQualityData: async () => reading },
        autoRetry: true,
      })
    );

    await client.readAndSubmitData();

    expect(submitDeviceData).toHaveBeenCalledTimes(1);
    expect(submitDeviceData).toHaveBeenCalledWith({
      deviceId: 'device-42',
      data: reading,
      timestamp: reading.timestamp,
      network: 'testnet',
    });
  });

  test('hazardous physical readings use the declared submitAlert SDK method', async () => {
    const submitAlert = jest.fn(async () => ({ accepted: true }));
    const client = registered(
      new AirQualityMonitorClient({
        sdk: {
          submitDeviceData: jest.fn(async () => ({ accepted: true })),
          submitAlert,
        } as never,
        adapter: {
          readAirQualityData: async () => ({
            pm25: 400,
            timestamp: '2026-09-17T12:00:00.000Z',
          }),
        },
      })
    );

    await client.readAndSubmitData();
    await new Promise(resolve => setImmediate(resolve));

    expect(submitAlert).toHaveBeenCalledTimes(1);
    expect(submitAlert).toHaveBeenCalledWith(
      expect.objectContaining({
        deviceId: 'device-42',
        type: 'AIR_QUALITY_ALERT',
      })
    );
  });

  test.each([
    ['array', []],
    ['no supported pollutants', { timestamp: '2026-09-17T12:00:00Z' }],
    ['non-numeric pollutant', { pm25: 'bad', timestamp: '2026-09-17T12:00:00Z' }],
    ['non-finite pollutant', { pm10: Number.NaN, timestamp: '2026-09-17T12:00:00Z' }],
    ['negative pollutant', { o3: -1, timestamp: '2026-09-17T12:00:00Z' }],
    ['out-of-range pollutant', { pm25: 501, timestamp: '2026-09-17T12:00:00Z' }],
    ['missing timestamp', { pm25: 1 }],
    ['invalid timestamp', { pm25: 1, timestamp: 'yesterday-ish' }],
  ])('rejects %s physical air-quality data before submission', async (_label, reading) => {
    const submitDeviceData = jest.fn();
    const client = registered(
      new AirQualityMonitorClient({
        sdk: { submitDeviceData, submitAlert: jest.fn() } as never,
        adapter: { readAirQualityData: async () => reading } as never,
      })
    );

    await expect(client.readAndSubmitData()).rejects.toThrow(
      /invalid|no supported|timestamp/
    );
    expect(submitDeviceData).not.toHaveBeenCalled();
  });

  test('firmware success requires a bounded nonempty version', async () => {
    const updateDeviceInfo = jest.fn();
    const updateDeviceStatus = jest.fn(async () => ({ ok: true }));
    const weather = registered(
      new WeatherStationClient({
        sdk: { updateDeviceInfo, updateDeviceStatus } as never,
        adapter: {
          readWeatherData: async () => ({ pressure: 1000 }),
          updateFirmware: async () => ({ success: true }),
        },
      })
    );

    await expect(
      weather.updateFirmware('file:///firmware.bin')
    ).rejects.toThrow('valid firmware version');
    expect(updateDeviceInfo).not.toHaveBeenCalled();
  });

  test('firmware refuses missing updateDeviceInfo before changing the device', async () => {
    const updateFirmware = jest.fn();
    const updateDeviceStatus = jest.fn();
    const weather = registered(
      new WeatherStationClient({
        sdk: { updateDeviceStatus } as never,
        adapter: { readWeatherData: async () => ({}), updateFirmware },
      })
    );

    await expect(weather.updateFirmware('file:///firmware.bin')).rejects.toThrow(
      'updateDeviceInfo'
    );
    expect(updateFirmware).not.toHaveBeenCalled();
  });

  test('weather status rejects non-finite and out-of-range battery levels', async () => {
    const updateDeviceStatus = jest.fn();
    const weather = registered(
      new WeatherStationClient({
        sdk: { updateDeviceStatus } as never,
        adapter: {
          readWeatherData: async () => ({}),
          readStatus: async () => ({
            isOnline: true,
            batteryLevel: Number.NaN,
            sensorStatus: { allOperational: true, issues: [] },
          }),
        },
      })
    );

    await expect(weather.checkStatus()).rejects.toThrow('incomplete status');
    expect(updateDeviceStatus).not.toHaveBeenCalled();
  });

  test.each([
    [
      'weather',
      WeatherStationClient,
      'readWeatherData',
      {
        isOnline: true,
        batteryLevel: 50,
        sensorStatus: { allOperational: false },
      },
    ],
    [
      'air quality',
      AirQualityMonitorClient,
      'readAirQualityData',
      {
        isOnline: true,
        calibrationStatus: { isCalibrated: true },
        sensorStatus: { allOperational: false },
      },
    ],
  ] as const)(
    '%s status requires bounded issue details for failed sensors',
    async (_label, Client, readMethod, status) => {
      const updateDeviceStatus = jest.fn();
      const client = registered(
        new Client({
          sdk: { updateDeviceStatus } as never,
          adapter: {
            [readMethod]: async () => ({}),
            readStatus: async () => status,
          } as never,
        })
      );

      await expect(client.checkStatus()).rejects.toThrow('incomplete status');
      expect(updateDeviceStatus).not.toHaveBeenCalled();
    }
  );

  test('weather status rejects oversized sensor issue details', async () => {
    const updateDeviceStatus = jest.fn();
    const weather = registered(
      new WeatherStationClient({
        sdk: { updateDeviceStatus } as never,
        adapter: {
          readWeatherData: async () => ({}),
          readStatus: async () => ({
            isOnline: true,
            batteryLevel: 50,
            sensorStatus: { allOperational: false, issues: ['x'.repeat(257)] },
          }),
        },
      })
    );

    await expect(weather.checkStatus()).rejects.toThrow('incomplete status');
    expect(updateDeviceStatus).not.toHaveBeenCalled();
  });

  test.each([undefined, '', 'x'.repeat(1025)])(
    'air status rejects invalid calibration message %p',
    async message => {
      const updateDeviceStatus = jest.fn();
      const air = registered(
        new AirQualityMonitorClient({
          sdk: { updateDeviceStatus } as never,
          adapter: {
            readAirQualityData: async () => ({}),
            readStatus: async () => ({
              isOnline: true,
              calibrationStatus: { isCalibrated: false, message },
              sensorStatus: { allOperational: true, issues: [] },
            }),
          },
        })
      );

      await expect(air.checkStatus()).rejects.toThrow('incomplete status');
      expect(updateDeviceStatus).not.toHaveBeenCalled();
    }
  );

  test.each([undefined, '', 'x'.repeat(1025)])(
    'firmware failure rejects invalid error detail %p',
    async error => {
      const updateDeviceStatus = jest.fn(async () => ({ ok: true }));
      const weather = registered(
        new WeatherStationClient({
          sdk: { updateDeviceInfo: jest.fn(), updateDeviceStatus } as never,
          adapter: {
            readWeatherData: async () => ({}),
            updateFirmware: async () => ({ success: false, error }),
          } as never,
        })
      );

      await expect(weather.updateFirmware('file:///firmware.bin')).rejects.toThrow(
        'invalid firmware error'
      );
      expect(updateDeviceStatus).not.toHaveBeenCalledWith(
        expect.objectContaining({ message: `Firmware update failed: ${error}` })
      );
    }
  );

  test('status strings are sanitized before chain-facing updates', async () => {
    const weatherStatus = jest.fn(async () => ({ ok: true }));
    const weather = registered(
      new WeatherStationClient({
        sdk: {
          updateDeviceInfo: jest.fn(),
          updateDeviceStatus: weatherStatus,
        } as never,
        adapter: {
          readWeatherData: async () => ({}),
          updateFirmware: async () => ({
            success: false,
            error: '  flash\n\tfailure  ',
          }),
        },
      })
    );
    await weather.updateFirmware('file:///firmware.bin');
    expect(weatherStatus).toHaveBeenCalledWith(
      expect.objectContaining({ message: 'Firmware update failed: flash failure' })
    );

    const airStatus = jest.fn(async () => ({ ok: true }));
    const air = registered(
      new AirQualityMonitorClient({
        sdk: { updateDeviceStatus: airStatus } as never,
        adapter: {
          readAirQualityData: async () => ({}),
          readStatus: async () => ({
            isOnline: true,
            calibrationStatus: {
              isCalibrated: false,
              message: '  recalibrate\n\tnow  ',
            },
            sensorStatus: { allOperational: true, issues: [] },
          }),
        },
      })
    );
    await air.checkStatus();
    expect(airStatus).toHaveBeenCalledWith(
      expect.objectContaining({ message: 'Calibration needed: recalibrate now' })
    );
  });

  test.each([
    ['weather', WeatherStationClient, 'readWeatherData'],
    ['air quality', AirQualityMonitorClient, 'readAirQualityData'],
  ] as const)(
    '%s start omits status timer when readStatus is absent',
    async (_label, Client, readMethod) => {
      jest.useFakeTimers();
      const client = registered(
        new Client({
          sdk: { submitDeviceData: jest.fn(async () => ({ ok: true })) } as never,
          adapter: {
            [readMethod]: async () => ({
              pm25: 1,
              humidity: 50,
              timestamp: '2026-09-17T12:00:00.000Z',
            }),
          } as never,
        })
      );

      await client.start();
      expect(
        (client as unknown as { statusCheckIntervalId: unknown })
          .statusCheckIntervalId
      ).toBeNull();
      client.stop();
      jest.useRealTimers();
    }
  );

  test('air calibration rejects a non-boolean success before restoring active', async () => {
    const updateDeviceStatus = jest.fn(async () => ({ ok: true }));
    const air = registered(
      new AirQualityMonitorClient({
        sdk: { updateDeviceStatus } as never,
        adapter: {
          readAirQualityData: async () => ({}),
          calibrate: async () => ({ success: 'false' }),
        } as never,
      })
    );

    await expect(air.calibrateSensors()).rejects.toThrow('invalid calibration result');
    expect(updateDeviceStatus).not.toHaveBeenCalledWith(
      expect.objectContaining({ status: 'active' })
    );
  });

  test.each([
    ['weather', WeatherStationClient, 'readWeatherData'],
    ['air quality', AirQualityMonitorClient, 'readAirQualityData'],
  ] as const)(
    '%s read failure preserves the original error and submits nothing',
    async (_label, Client, method) => {
      const original = new Error('physical bus disconnected');
      const submitDeviceData = jest.fn();
      const adapter = { [method]: async () => Promise.reject(original) };
      const client = registered(
        new Client({
          sdk: { submitDeviceData } as never,
          adapter: adapter as never,
          autoRetry: true,
        })
      );

      await expect(client.readAndSubmitData()).rejects.toBe(original);
      expect(submitDeviceData).not.toHaveBeenCalled();
      expect(
        (client as unknown as { offlineQueue: unknown[] }).offlineQueue
      ).toEqual([]);
    }
  );

  test('failed submission queues the original adapter reading once', async () => {
    const reading = {
      pm25: 2,
      pm10: 4,
      o3: 8,
      timestamp: '2026-09-17T12:00:00.000Z',
    };
    const original = new Error('ingest refused payload');
    const client = registered(
      new AirQualityMonitorClient({
        sdk: {
          submitDeviceData: jest.fn(async () => Promise.reject(original)),
        } as never,
        adapter: { readAirQualityData: async () => reading },
        autoRetry: true,
      })
    );

    await expect(client.readAndSubmitData()).rejects.toBe(original);
    const queue = (client as unknown as { offlineQueue: unknown[] })
      .offlineQueue;
    expect(queue).toHaveLength(1);
    expect(queue[0]).toEqual(
      expect.objectContaining({ type: 'data', data: reading })
    );
  });

  test.each([
    ['weather', () => new WeatherStationClient({ sdk: { submitDeviceData: jest.fn() } as never, adapter: { readWeatherData: async () => ({}) }, autoRetry: true }), { temperature_c: 20 }],
    ['air-quality', () => new AirQualityMonitorClient({ sdk: { submitDeviceData: jest.fn() } as never, adapter: { readAirQualityData: async () => ({}) }, autoRetry: true }), { pm25: 2, pm10: 4, o3: 8 }],
  ])('%s submitData refuses a reading with no observation timestamp instead of inventing one', async (_label, make, reading) => {
    const client = registered(make() as never) as unknown as {
      submitData(data: unknown): Promise<unknown>;
      sdk: { submitDeviceData: jest.Mock };
      offlineQueue: unknown[];
    };
    await expect(client.submitData(reading)).rejects.toThrow(/observation timestamp/);
    expect(client.sdk.submitDeviceData).not.toHaveBeenCalled();
    // a reading that can never be valid is not queued for endless retry
    expect(client.offlineQueue).toHaveLength(0);
  });

  test('air-quality submitData keeps the adapter timestamp verbatim', async () => {
    const submitDeviceData = jest.fn(async () => ({ accepted: true }));
    const client = registered(
      new AirQualityMonitorClient({
        sdk: { submitDeviceData } as never,
        adapter: { readAirQualityData: async () => ({}) },
      })
    );
    const reading = { pm25: 2, pm10: 4, o3: 8, timestamp: '2026-09-17T12:00:00.000Z' };
    await (client as unknown as { submitData(d: unknown): Promise<unknown> }).submitData(reading);
    expect(submitDeviceData).toHaveBeenCalledWith(
      expect.objectContaining({ timestamp: '2026-09-17T12:00:00.000Z' })
    );
  });

  test('air-quality submitAlert refuses an alert with no observation timestamp instead of inventing one', async () => {
    const submitAlert = jest.fn(async () => ({ accepted: true }));
    const client = registered(
      new AirQualityMonitorClient({
        sdk: { submitAlert } as never,
        adapter: { readAirQualityData: async () => ({}) },
        autoRetry: true,
      })
    ) as unknown as {
      submitAlert(a: unknown): Promise<unknown>;
      offlineQueue: unknown[];
    };
    const alert = { pollutant: 'pm25', value: 80, level: 'unhealthy' };
    await expect(client.submitAlert(alert)).rejects.toThrow(/observation timestamp/);
    await expect(client.submitAlert({ ...alert, timestamp: '' })).rejects.toThrow(/observation timestamp/);
    expect(submitAlert).not.toHaveBeenCalled();
    expect(client.offlineQueue).toHaveLength(0);
    // a real observation time is sent verbatim
    await client.submitAlert({ ...alert, timestamp: '2026-09-17T12:00:00.000Z' });
    expect(submitAlert).toHaveBeenCalledWith(
      expect.objectContaining({
        alert: expect.objectContaining({ timestamp: '2026-09-17T12:00:00.000Z' }),
      })
    );
  });

  test.each([
    ['weather', () => new WeatherStationClient({ sdk: { submitDeviceData: jest.fn(async () => { throw new Error('transport down'); }) } as never, adapter: { readWeatherData: async () => ({}) }, autoRetry: true }), { temperature_c: 20, timestamp: 1_800_000_000 }],
    ['air-quality', () => new AirQualityMonitorClient({ sdk: { submitDeviceData: jest.fn(async () => { throw new Error('transport down'); }) } as never, adapter: { readAirQualityData: async () => ({}) }, autoRetry: true }), { pm25: 2, pm10: 4, o3: 8, timestamp: '2026-09-17T12:00:00.000Z' }],
  ])('%s offline queue drops a nonRetryable entry but keeps a transient one', async (_label, make, reading) => {
    const client = registered(make() as never) as unknown as {
      submitData(data: unknown): Promise<unknown>;
      processOfflineQueue(): Promise<unknown>;
      offlineQueue: { type: string; data: Record<string, unknown> }[];
    };
    // transient transport failure: the valid reading is queued for retry
    await expect(client.submitData(reading)).rejects.toThrow('transport down');
    expect(client.offlineQueue.length).toBeGreaterThan(0);
    // still transient on the drain: the entry stays queued
    await client.processOfflineQueue();
    expect(client.offlineQueue.length).toBeGreaterThan(0);
    // the retained reading loses its observation time: the refusal is permanent, so it leaves the queue
    for (const item of client.offlineQueue) {
      delete item.data.timestamp;
      delete item.data.observed_at_unix;
    }
    await client.processOfflineQueue();
    await client.processOfflineQueue();
    expect(client.offlineQueue).toHaveLength(0);
  });

  test('status, calibration, and firmware operations fail without adapter support', async () => {
    const updateDeviceStatus = jest.fn(async () => ({ ok: true }));
    const weather = registered(
      new WeatherStationClient({
        sdk: { updateDeviceStatus, updateDeviceInfo: jest.fn() } as never,
        adapter: { readWeatherData: async () => ({ pressure: 1000 }) },
      })
    );
    const air = registered(
      new AirQualityMonitorClient({
        sdk: { updateDeviceStatus } as never,
        adapter: { readAirQualityData: async () => ({ pm25: 1 }) },
      })
    );

    await expect(weather.checkStatus()).rejects.toThrow('readStatus');
    await expect(
      weather.updateFirmware('file:///firmware.bin')
    ).rejects.toThrow('updateFirmware');
    await expect(air.checkStatus()).rejects.toThrow('readStatus');
    await expect(air.calibrateSensors()).rejects.toThrow('calibrate');
  });
});
