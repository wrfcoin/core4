import type { PhysicalDeviceAdapter } from './PhysicalDeviceAdapter.js';

export interface WeatherStationOptions {
  sdk: {
    registerIoTDevice(data: unknown): Promise<{ id: string }>;
    submitDeviceData(data: unknown): Promise<unknown>;
    updateDeviceStatus?(data: unknown): Promise<unknown>;
    updateDeviceInfo(data: unknown): Promise<unknown>;
  };
  adapter: PhysicalDeviceAdapter & {
    readWeatherData(): Promise<Record<string, unknown>>;
  };
  deviceInfo?: Record<string, unknown>;
  updateInterval?: number;
  network?: string;
  maxQueueSize?: number;
  autoRetry?: boolean;
  retryDelay?: number;
}

export declare class WeatherStationClient {
  constructor(options: WeatherStationOptions);
  register(): Promise<unknown>;
  start(): Promise<void>;
  stop(): void;
  readAndSubmitData(): Promise<unknown>;
  submitData(data: Record<string, unknown>): Promise<unknown>;
  checkStatus(): Promise<unknown>;
  updateFirmware(firmwareUrl: string): Promise<unknown>;
  processOfflineQueue(): Promise<unknown>;
}

export default WeatherStationClient;
