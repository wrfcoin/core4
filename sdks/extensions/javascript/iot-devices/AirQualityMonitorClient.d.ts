import type { PhysicalDeviceAdapter } from './PhysicalDeviceAdapter.js';

export interface AirQualityMonitorOptions {
  sdk: {
    registerIoTDevice(data: unknown): Promise<{ id: string }>;
    submitDeviceData(data: unknown): Promise<unknown>;
    updateDeviceStatus?(data: unknown): Promise<unknown>;
    submitAlert(data: unknown): Promise<unknown>;
  };
  adapter: PhysicalDeviceAdapter & {
    readAirQualityData(): Promise<Record<string, unknown>>;
  };
  deviceInfo?: Record<string, unknown>;
  updateInterval?: number;
  network?: string;
  maxQueueSize?: number;
  autoRetry?: boolean;
  retryDelay?: number;
}

export declare class AirQualityMonitorClient {
  constructor(options: AirQualityMonitorOptions);
  register(): Promise<unknown>;
  start(): Promise<void>;
  stop(): void;
  readAndSubmitData(): Promise<unknown>;
  submitData(data: Record<string, unknown>): Promise<unknown>;
  checkStatus(): Promise<unknown>;
  calibrateSensors(): Promise<unknown>;
  processOfflineQueue(): Promise<unknown>;
}

export default AirQualityMonitorClient;
