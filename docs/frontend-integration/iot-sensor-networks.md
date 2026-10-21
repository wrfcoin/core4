# IoT Sensor Network Integration Guide

## Overview

The backend supports 20+ IoT sensor platform integrations through the IoT Sensor Networks Server
(port 3002), but none of these are currently exposed in the frontend.

## Backend Capabilities

### Supported Sensor Platforms

- **Environmental Monitoring**: Sensor Community, Luftdaten, OpenAQ, AirNow, EPA, Weatherbit,
  OpenWeather
- **IoT Device Networks**: Helium, IoTeX, TheThingsNetwork, Sigfox, LoRaWAN
- **Specialized Sensors**: Seismic, Radiation, Water Quality, Air Quality, Soil, Noise, Traffic,
  Energy

### Available Endpoints

- `POST /sensors/aggregate-data` - Aggregate data from multiple platforms
- `GET /sensors/stream/:location` - Real-time sensor data streaming
- `POST /sensors/register-device` - Register and attest new devices
- `POST /sensors/iotex/submit` - Submit data to IoTeX blockchain
- `POST /sensors/helium/register` - Register devices on Helium network

## Frontend Integration Requirements

### 1. API Service Extension

Add to `frontend/src/services/api.ts`:

```typescript
// IoT Sensor Types
interface SensorPlatform {
  id: string;
  name: string;
  type: 'environmental' | 'iot' | 'specialized';
  supportedDataTypes: string[];
  streamingSupported: boolean;
  blockchainIntegration?: string;
}

interface SensorData {
  platform: string;
  deviceId: string;
  location: {
    lat: number;
    lng: number;
  };
  timestamp: Date;
  measurements: Record<string, number>;
  qualityScore: number;
  metadata?: Record<string, any>;
}

interface AggregatedSensorData {
  location: {
    lat: number;
    lng: number;
  };
  timestamp: Date;
  consensus: Record<string, number>;
  individual: SensorData[];
  platforms: {
    successful: number;
    failed: number;
    failureDetails: Array<{ platform: string; error: string }>;
  };
  qualityScore: number;
}

interface DeviceRegistration {
  deviceId: string;
  deviceType: string;
  location: {
    lat: number;
    lng: number;
  };
  capabilities: string[];
  publicKey: string;
  attestationSignature: string;
}

// IoT Sensor API namespace
export const sensorAPI = {
  // Aggregate data from multiple platforms
  aggregateData: async (params: {
    location: { lat: number; lng: number };
    radius: number;
    dataTypes: string[];
    timeRange: { start: Date; end: Date };
    platforms: string[];
  }): Promise<AggregatedSensorData> => {
    return await api.post('/sensors/aggregate-data', params).then(r => r.data);
  },

  // Real-time sensor streaming
  streamSensorData: (location: string, platforms?: string[], dataTypes?: string[]) => {
    const params = new URLSearchParams();
    if (platforms) params.append('platforms', platforms.join(','));
    if (dataTypes) params.append('dataTypes', dataTypes.join(','));

    return new EventSource(`${getBaseURL()}/sensors/stream/${location}?${params}`);
  },

  // Register new device
  registerDevice: async (
    registration: DeviceRegistration
  ): Promise<{
    registration: any;
    credentials: any;
    networkInfo: any;
  }> => {
    return await api.post('/sensors/register-device', registration).then(r => r.data);
  },

  // IoTeX submission
  submitToIoTeX: async (params: {
    deviceId: string;
    sensorData: any;
    proof: string;
  }): Promise<{
    submission: any;
    qualityScore: number;
    reward: number;
    blockchainTx: string;
  }> => {
    return await api.post('/sensors/iotex/submit', params).then(r => r.data);
  },

  // Helium registration
  registerHeliumDevice: async (params: {
    hotspotAddress: string;
    deviceEui: string;
    appEui: string;
    devAddr: string;
  }): Promise<any> => {
    return await api.post('/sensors/helium/register', params).then(r => r.data);
  },
};
```

### 2. React Components Needed

#### SensorNetworkDashboard Component

- Platform selection interface
- Real-time data aggregation display
- Quality score visualization
- Platform health monitoring
- Error handling for failed platforms

#### SensorMapView Component

- Interactive map with sensor locations
- Real-time data overlay
- Clustering for dense areas
- Click for detailed sensor info
- Filter by platform and data type

#### DeviceRegistrationFlow Component

- Multi-step registration wizard
- Device attestation interface
- QR code scanning for credentials
- Network selection (Helium, IoTeX, etc.)
- Credential storage guidance

#### SensorDataStream Component

- Real-time data visualization
- Multiple chart types (line, gauge, heatmap)
- Platform comparison view
- Data quality indicators
- Alert thresholds

### 3. State Management

```typescript
// Sensor network state interface
interface SensorNetworkState {
  platforms: SensorPlatform[];
  activeSensors: Map<string, SensorData>;
  aggregatedData: AggregatedSensorData | null;
  registeredDevices: DeviceRegistration[];
  streamConnections: Map<string, EventSource>;
  dataHistory: SensorData[];
  alerts: SensorAlert[];
  isLoading: boolean;
  error: string | null;
}

// Sensor alert type
interface SensorAlert {
  id: string;
  severity: 'low' | 'medium' | 'high' | 'critical';
  platform: string;
  deviceId?: string;
  message: string;
  timestamp: Date;
  location?: { lat: number; lng: number };
}
```

### 4. Real-Time Integration

```typescript
// Hook for sensor streaming
const useSensorStream = (location: string, platforms?: string[]) => {
  const [data, setData] = useState<SensorData[]>([]);
  const [isConnected, setIsConnected] = useState(false);

  useEffect(() => {
    const eventSource = sensorAPI.streamSensorData(location, platforms);

    eventSource.onopen = () => setIsConnected(true);

    eventSource.onmessage = event => {
      const sensorData = JSON.parse(event.data);
      setData(prev => [...prev.slice(-100), sensorData]); // Keep last 100
    };

    eventSource.onerror = () => setIsConnected(false);

    return () => eventSource.close();
  }, [location, platforms]);

  return { data, isConnected };
};
```

### 5. UI/UX Requirements

#### Platform Selection

- Grid view of available platforms
- Filter by type (environmental, IoT, specialized)
- Show platform status and data quality
- Multi-select with select all option

#### Data Visualization

- Real-time gauges for current values
- Historical charts with zoom/pan
- Comparison charts across platforms
- Heatmaps for geographic distribution
- Data quality badges

#### Device Management

- Device list with status indicators
- Remote configuration options
- Firmware update notifications
- Performance metrics per device
- Earnings/rewards display

### 6. Security Considerations

- Validate device attestation signatures
- Secure storage of device credentials
- Rate limiting for data submissions
- Anomaly detection for suspicious data
- Access control for device management

## Implementation Priority

1. Basic sensor data display with platform selection
2. Real-time streaming interface
3. Map-based visualization
4. Device registration flow
5. Blockchain integration (IoTeX, Helium)
6. Advanced analytics and alerts

## Testing Requirements

- Mock sensor data for development
- Test with multiple platforms simultaneously
- Validate real-time performance
- Test error handling for platform failures
- Performance test with high-frequency data
- Test device registration flows
