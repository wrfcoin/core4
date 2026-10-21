# WrfML Data Format

## Overview

WrfML (Weather Reporting Format Markup Language) is Wrfcoin's standardized data format for weather
measurements, environmental data, and computational results. This format ensures consistent data
structure across the entire Wrfcoin network while maintaining compatibility with existing weather
data standards.

## Core Structure

### Base Data Schema

```json
{
  "wrfml_version": "1.2.0",
  "metadata": {
    "timestamp": "2024-01-15T10:30:00.000Z",
    "format_type": "weather_measurement",
    "encoding": "utf-8",
    "checksum": "sha256:abc123...",
    "schema_version": "1.2"
  },
  "header": {
    "station_id": "WRF_STATION_001",
    "location": {
      "latitude": 40.7128,
      "longitude": -74.006,
      "elevation": 10.0,
      "timezone": "UTC-5"
    },
    "device_info": {
      "manufacturer": "WeatherTech",
      "model": "WT-Pro-2024",
      "firmware_version": "2.1.4",
      "calibration_date": "2024-01-01T00:00:00.000Z"
    }
  },
  "data": {
    // Measurement data goes here
  },
  "validation": {
    "signature": "0x1234567890abcdef...",
    "validator_count": 5,
    "consensus_score": 0.98
  }
}
```

## Data Types

### Weather Measurements

```json
{
  "data": {
    "temperature": {
      "value": 23.5,
      "unit": "celsius",
      "precision": 0.1,
      "confidence": 0.95
    },
    "humidity": {
      "value": 65.2,
      "unit": "percent",
      "precision": 0.1,
      "confidence": 0.92
    },
    "pressure": {
      "value": 1013.25,
      "unit": "hPa",
      "precision": 0.01,
      "confidence": 0.99
    },
    "wind": {
      "speed": {
        "value": 12.5,
        "unit": "km/h",
        "precision": 0.1
      },
      "direction": {
        "value": 225,
        "unit": "degrees",
        "precision": 1
      },
      "gust": {
        "value": 18.3,
        "unit": "km/h",
        "precision": 0.1
      }
    },
    "precipitation": {
      "rate": {
        "value": 2.5,
        "unit": "mm/h",
        "precision": 0.1
      },
      "accumulation": {
        "value": 12.7,
        "unit": "mm",
        "period": "24h"
      }
    }
  }
}
```

### Computational Results

```json
{
  "data": {
    "computation_type": "weather_forecast",
    "model_info": {
      "name": "WRF-Advanced-v2.1",
      "version": "2.1.0",
      "parameters": {
        "resolution": "1km",
        "time_step": "10s",
        "forecast_hours": 72
      }
    },
    "input_data": {
      "sources": ["station_001", "station_002", "satellite_data"],
      "time_range": {
        "start": "2024-01-15T06:00:00.000Z",
        "end": "2024-01-15T18:00:00.000Z"
      }
    },
    "results": {
      "forecast": [
        {
          "timestamp": "2024-01-16T12:00:00.000Z",
          "temperature": 25.3,
          "humidity": 68.5,
          "precipitation_probability": 0.15
        }
      ],
      "confidence_metrics": {
        "overall_confidence": 0.87,
        "temperature_accuracy": 0.92,
        "precipitation_accuracy": 0.78
      }
    }
  }
}
```

### Sensor Network Data

```json
{
  "data": {
    "network_info": {
      "network_id": "WRF_MESH_NYC_001",
      "node_count": 25,
      "coverage_area": {
        "type": "polygon",
        "coordinates": [
          [-74.02, 40.7],
          [-74.0, 40.7],
          [-74.0, 40.72],
          [-74.02, 40.72],
          [-74.02, 40.7]
        ]
      }
    },
    "aggregated_data": {
      "temperature": {
        "average": 23.8,
        "minimum": 21.2,
        "maximum": 26.4,
        "standard_deviation": 1.2
      },
      "data_quality": {
        "completeness": 0.96,
        "accuracy": 0.94,
        "timeliness": 0.98
      }
    },
    "individual_nodes": [
      {
        "node_id": "WRF_NODE_001",
        "location": [-74.01, 40.71],
        "measurements": {
          "temperature": 23.5,
          "humidity": 65.2
        },
        "status": "active",
        "last_update": "2024-01-15T10:29:45.000Z"
      }
    ]
  }
}
```

## Validation Schema

### Data Integrity Checks

```javascript
class WrfMLValidator {
  validateFormat(wrfmlData) {
    const schema = {
      wrfml_version: { required: true, type: 'string' },
      metadata: {
        required: true,
        properties: {
          timestamp: { required: true, type: 'iso8601' },
          format_type: {
            required: true,
            enum: ['weather_measurement', 'computation_result', 'sensor_network'],
          },
          checksum: { required: true, pattern: /^sha256:[a-f0-9]{64}$/ },
        },
      },
      header: { required: true, type: 'object' },
      data: { required: true, type: 'object' },
      validation: { required: true, type: 'object' },
    };

    return this.validateAgainstSchema(wrfmlData, schema);
  }

  validateChecksum(wrfmlData) {
    const dataString = JSON.stringify(wrfmlData.data);
    const calculatedChecksum = crypto.createHash('sha256').update(dataString).digest('hex');
    const providedChecksum = wrfmlData.metadata.checksum.replace('sha256:', '');

    return calculatedChecksum === providedChecksum;
  }

  validateSignature(wrfmlData) {
    const messageHash = this.calculateMessageHash(wrfmlData);
    const signature = wrfmlData.validation.signature;

    return this.verifyBlockchainSignature(messageHash, signature);
  }
}
```

## Encoding and Compression

### Binary Encoding (WrfMLB)

```javascript
class WrfMLBinaryEncoder {
  encode(wrfmlData) {
    const buffer = new ArrayBuffer(1024); // Initial size
    const view = new DataView(buffer);
    let offset = 0;

    // Header: Magic number (4 bytes) + Version (2 bytes)
    view.setUint32(offset, 0x57524642); // 'WRFB'
    offset += 4;
    view.setUint16(offset, 0x0102); // Version 1.2
    offset += 2;

    // Encode metadata
    offset = this.encodeMetadata(view, offset, wrfmlData.metadata);

    // Encode header
    offset = this.encodeHeader(view, offset, wrfmlData.header);

    // Encode data
    offset = this.encodeData(view, offset, wrfmlData.data);

    return buffer.slice(0, offset);
  }

  decode(binaryData) {
    const view = new DataView(binaryData);
    let offset = 0;

    // Verify magic number
    const magic = view.getUint32(offset);
    if (magic !== 0x57524642) {
      throw new Error('Invalid WrfMLB format');
    }
    offset += 4;

    // Read version
    const version = view.getUint16(offset);
    offset += 2;

    // Decode sections
    const metadata = this.decodeMetadata(view, offset);
    offset = metadata.nextOffset;

    const header = this.decodeHeader(view, offset);
    offset = header.nextOffset;

    const data = this.decodeData(view, offset);

    return {
      wrfml_version: `${version >> 8}.${version & 0xff}.0`,
      metadata: metadata.data,
      header: header.data,
      data: data.data,
    };
  }
}
```

## Interoperability

### Standard Format Conversion

```javascript
class WrfMLConverter {
  // Convert to METAR format
  toMETAR(wrfmlData) {
    const data = wrfmlData.data;
    const location = wrfmlData.header.location;
    const timestamp = new Date(wrfmlData.metadata.timestamp);

    const metar = [
      `METAR ${wrfmlData.header.station_id}`,
      `${this.formatDateTime(timestamp)}`,
      `${this.formatWind(data.wind)}`,
      `${this.formatVisibility(data.visibility)}`,
      `${this.formatTemperature(data.temperature, data.humidity)}`,
      `${this.formatPressure(data.pressure)}`,
    ].join(' ');

    return metar;
  }

  // Convert to JSON-LD
  toJSONLD(wrfmlData) {
    return {
      '@context': {
        '@vocab': 'https://schema.org/',
        wrf: 'https://wrfcoin.network/schema/',
        geo: 'http://www.w3.org/2003/01/geo/wgs84_pos#',
      },
      '@type': 'WeatherObservation',
      identifier: wrfmlData.header.station_id,
      dateObserved: wrfmlData.metadata.timestamp,
      location: {
        '@type': 'GeoCoordinates',
        latitude: wrfmlData.header.location.latitude,
        longitude: wrfmlData.header.location.longitude,
      },
      temperature: {
        '@type': 'QuantitativeValue',
        value: wrfmlData.data.temperature.value,
        unitCode: 'CEL',
      },
      relativeHumidity: {
        '@type': 'QuantitativeValue',
        value: wrfmlData.data.humidity.value,
        unitCode: 'P1',
      },
    };
  }

  // Convert from BUFR format
  fromBUFR(bufrData) {
    const parsed = this.parseBUFR(bufrData);

    return {
      wrfml_version: '1.2.0',
      metadata: {
        timestamp: parsed.timestamp,
        format_type: 'weather_measurement',
        encoding: 'utf-8',
        source_format: 'BUFR',
      },
      header: {
        station_id: parsed.stationId,
        location: {
          latitude: parsed.latitude,
          longitude: parsed.longitude,
          elevation: parsed.elevation,
        },
      },
      data: this.mapBUFRToWrfML(parsed.observations),
    };
  }
}
```

## API Integration

### RESTful API Support

```javascript
// POST /api/v1/data/submit
const submitWeatherData = async wrfmlData => {
  const response = await fetch('/api/v1/data/submit', {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      'WrfML-Version': '1.2.0',
    },
    body: JSON.stringify(wrfmlData),
  });

  return response.json();
};

// GET /api/v1/data/query
const queryWeatherData = async filters => {
  const params = new URLSearchParams({
    format: 'wrfml',
    version: '1.2.0',
    ...filters,
  });

  const response = await fetch(`/api/v1/data/query?${params}`);
  return response.json();
};
```

## Real-Time Streaming

### WebSocket Protocol

```javascript
class WrfMLStream {
  constructor(endpoint) {
    this.ws = new WebSocket(endpoint);
    this.setupHandlers();
  }

  setupHandlers() {
    this.ws.onmessage = event => {
      const wrfmlData = JSON.parse(event.data);
      this.handleWeatherUpdate(wrfmlData);
    };
  }

  subscribe(filters) {
    const subscription = {
      type: 'subscribe',
      format: 'wrfml',
      version: '1.2.0',
      filters: filters,
    };

    this.ws.send(JSON.stringify(subscription));
  }

  handleWeatherUpdate(wrfmlData) {
    // Validate incoming data
    if (this.validator.validateFormat(wrfmlData)) {
      this.onDataReceived(wrfmlData);
    }
  }
}
```

## Use Cases

1. **Weather Station Data**: Standardized sensor readings
2. **Computational Results**: AI/ML model outputs
3. **Satellite Data**: Remote sensing measurements
4. **IoT Networks**: Distributed sensor arrays
5. **Research Data**: Academic weather studies
6. **Commercial APIs**: Third-party integrations

## Related Standards

- **CF Conventions**: Climate and Forecast metadata
- **OGC SensorML**: Sensor model language
- **WMO BUFR**: Binary Universal Form for Representation
- **JSON-LD**: Linked data format
- **METAR/TAF**: Aviation weather formats
