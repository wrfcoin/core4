# WRFCoin SDK Extensions

This directory contains custom functionality that extends the auto-generated SDKs.

## JavaScript Extensions

### Weather Validation (`javascript/weather-validation/`)

Advanced weather data quality validation and processing:

```typescript
import { WeatherService } from '@wrfcoin/sdk-extensions';

const weatherService = new WeatherService(graphqlClient);

// Validate weather data quality
const validation = weatherService.validateDataQuality(weatherData);
console.log(`Quality Score: ${validation.qualityScore}%`);

// Calculate potential rewards
const rewards = weatherService.calculatePotentialRewards(95, 10);
```

### IoT Devices (`javascript/iot-devices/`)

IoT device management and integration:

```typescript
import { IoTDeviceManager, WeatherStationClient } from '@wrfcoin/sdk-extensions';

const deviceManager = new IoTDeviceManager({ sdk });
const station = new WeatherStationClient({
  sdk,
  adapter: physicalWeatherStationAdapter,
  deviceInfo: { name: 'station-001' },
});

// Connect and manage IoT devices
await deviceManager.connectDevice(station);
const data = await station.readWeatherData();
```

## Retired component experiments

The trees under `components/react/` and `components/vue/` are unsupported, non-production examples.
They are not installable packages, are not exported by release artifacts, and may contain incomplete
or simulated behavior. See each tree's README for the retirement notice.

## Installation

### JavaScript Extensions

```bash
cd sdks/extensions/javascript
npm install
npm run build
```

## Usage with Generated SDKs

Extensions are designed to work alongside the auto-generated SDKs:

```typescript
// Use generated SDK for basic API calls
import { WeatherDataApi, Configuration } from '@wrfcoin/sdk-typescript';

// Use extensions for advanced features
import { WeatherService } from '@wrfcoin/sdk-extensions';

const config = new Configuration({ apiKey: 'your-key' });
const weatherApi = new WeatherDataApi(config);
const weatherService = new WeatherService(graphqlClient);

// Submit data via generated API
const response = await weatherApi.apiWeatherSubmitPost(data);

// Validate quality via extensions
const quality = weatherService.validateDataQuality(data);
```

## Contributing

When adding new extensions:

1. Place JavaScript/TypeScript code in `javascript/` subdirectories
2. Do not add release claims or package exports for the retired component trees
3. Update this README with usage examples
4. Ensure compatibility with generated SDKs
5. Add appropriate tests and documentation
