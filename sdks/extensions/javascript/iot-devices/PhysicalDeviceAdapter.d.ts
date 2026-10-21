export interface DeviceSensorStatus {
  allOperational: boolean;
  issues: string[];
}

export interface WeatherDeviceStatus {
  isOnline: boolean;
  batteryLevel: number;
  sensorStatus: DeviceSensorStatus;
}

export interface AirQualityDeviceStatus {
  isOnline: boolean;
  calibrationStatus: { isCalibrated: boolean; message?: string };
  sensorStatus: DeviceSensorStatus;
}

/** Implemented by physical-device integrations outside the SDK. */
export interface PhysicalDeviceAdapter {
  readWeatherData?(): Promise<Record<string, unknown>>;
  readAirQualityData?(): Promise<Record<string, unknown>>;
  readStatus?(): Promise<WeatherDeviceStatus | AirQualityDeviceStatus>;
  updateFirmware?(firmwareUrl: string): Promise<{
    success: boolean;
    version?: string;
    error?: string;
  }>;
  calibrate?(): Promise<{
    success: boolean;
    calibratedSensors?: string[];
    error?: string;
  }>;
}
