/**
 * Type declarations for IoTDeviceManager JavaScript module
 */

export interface DeviceInfo {
  id: string;
  type: 'weather_station' | 'air_quality_monitor';
  status: 'online' | 'offline' | 'error';
  lastSeen: Date;
}

export interface ManagerConfig {
  apiKey: string;
  endpoint?: string;
  pollInterval?: number;
}

export declare class IoTDeviceManager {
  constructor(config: ManagerConfig);

  initialize(): Promise<void>;
  registerDevice(deviceInfo: DeviceInfo): Promise<void>;
  unregisterDevice(deviceId: string): Promise<void>;
  getDevices(): Promise<DeviceInfo[]>;
  getDevice(deviceId: string): Promise<DeviceInfo | null>;
}

export default IoTDeviceManager;
