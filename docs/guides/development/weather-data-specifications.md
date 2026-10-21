# Weather Data Format Specifications

This guide provides detailed specifications for the weather data formats used in the Wrfcoin
platform, including standards for data exchange, validation, and storage.

## Table of Contents

1. [Introduction](#introduction)
2. [Core Weather Data Types](#core-weather-data-types)
3. [Station Data Format](#station-data-format)
4. [Grid Data Format](#grid-data-format)
5. [Weather Forecast Format](#weather-forecast-format)
6. [Climate Data Format](#climate-data-format)
7. [Data Quality Indicators](#data-quality-indicators)
8. [Data Encoding in Transactions](#data-encoding-in-transactions)
9. [Blockchain Storage Model](#blockchain-storage-model)
10. [Computational Data Blocks](#computational-data-blocks)

## Introduction

Wrfcoin uses specialized data formats optimized for blockchain storage and verification. These
formats balance:

- **Precision**: Ensuring accuracy of meteorological measurements
- **Compactness**: Minimizing blockchain storage requirements
- **Verifiability**: Enabling validation by network nodes
- **Extensibility**: Supporting future data types and measurements

All weather data in Wrfcoin adheres to the Weather-on-Chain (WoC) standard, which defines the
structure, encoding, and validation rules.

## Core Weather Data Types

### Base Measurement Types

| Type            | Format | Resolution | Range             | Units |
| --------------- | ------ | ---------- | ----------------- | ----- |
| Temperature     | int16  | 0.1        | -327.68 to 327.67 | °C    |
| Humidity        | uint8  | 1.0        | 0 to 100          | %     |
| Pressure        | uint16 | 0.1        | 0 to 6553.5       | hPa   |
| Wind Speed      | uint16 | 0.1        | 0 to 6553.5       | m/s   |
| Wind Direction  | uint16 | 0.1        | 0 to 359.9        | °     |
| Precipitation   | uint16 | 0.1        | 0 to 6553.5       | mm    |
| Solar Radiation | uint16 | 1.0        | 0 to 65535        | W/m²  |
| UV Index        | uint8  | 0.1        | 0 to 25.5         | -     |
| Cloud Cover     | uint8  | 1.0        | 0 to 100          | %     |
| Visibility      | uint16 | 10         | 0 to 655350       | m     |

### Coordinates Format

Geographic coordinates are stored as signed integers with 6 decimal precision:

```javascript
// Example: 40.7128° N, 74.0060° W (New York City)
const latitude = 40.7128 * 1000000; // 40712800
const longitude = -74.006 * 1000000; // -74006000
```

### Timestamp Format

Timestamps are stored as UNIX timestamps (seconds since January 1, 1970, UTC):

```javascript
const timestamp = Math.floor(Date.now() / 1000);
```

## Station Data Format

Weather station data includes both metadata about the station and the measurements collected.

### Station Metadata

```json
{
  "id": "WS-NYC-001",
  "type": "official", // official, amateur, research, iot
  "location": {
    "latitude": 40712800,
    "longitude": -74006000,
    "elevation": 10, // meters above sea level
    "name": "Central Park Weather Station"
  },
  "instruments": [
    {
      "type": "temperature",
      "model": "Davis VP2 Temp-Hum",
      "accuracy": 0.5,
      "lastCalibration": 1672531200 // Jan 1, 2023
    },
    {
      "type": "humidity",
      "model": "Davis VP2 Temp-Hum",
      "accuracy": 3.0,
      "lastCalibration": 1672531200
    }
  ],
  "reportingInterval": 300, // seconds
  "owner": "NYC-Met-Dept",
  "commissionDate": 1577836800, // Jan 1, 2020
  "status": "active" // active, maintenance, decommissioned
}
```

### Station Measurement Data

```json
{
  "stationId": "WS-NYC-001",
  "timestamp": 1683112800, // May 3, 2023, 12:00 UTC
  "measurements": {
    "temperature": 225, // 22.5°C
    "humidity": 65, // 65%
    "pressure": 10132, // 1013.2 hPa
    "windSpeed": 123, // 12.3 m/s
    "windDirection": 2700, // 270.0°
    "precipitation": 0, // 0.0 mm
    "solarRadiation": 850, // 850 W/m²
    "uvIndex": 68, // 6.8
    "cloudCover": 30, // 30%
    "visibility": 2000 // 20,000 m
  },
  "quality": {
    "flags": 0, // 0 = no quality issues
    "confidence": 95 // 95% confidence
  },
  "hash": "0x7a61..." // Hash of all measurement data for verification
}
```

## Grid Data Format

Gridded data represents weather data over a geographic area, typically from numerical models.

### Grid Metadata

```json
{
  "id": "GRD-ECM-GLOBAL-025",
  "source": "ECMWF", // Data source or model
  "type": "analysis", // analysis, forecast
  "creationTime": 1683112800, // When the grid was produced
  "validTime": 1683112800, // Time the data represents
  "resolution": {
    "spatial": 0.25, // Degrees
    "temporal": 3600 // Seconds
  },
  "projection": "WGS84", // Coordinate reference system
  "parameters": ["temperature", "humidity", "pressure"],
  "coverage": {
    "north": 90.0,
    "south": -90.0,
    "east": 180.0,
    "west": -180.0
  },
  "levels": [
    {
      "type": "pressure",
      "value": 1000, // hPa
      "parameters": ["temperature", "humidity"]
    },
    {
      "type": "pressure",
      "value": 850, // hPa
      "parameters": ["temperature", "humidity"]
    }
  ]
}
```

### Grid Point Data

Grid data can be stored in two formats:

#### 1. Full Grid (all points)

```json
{
  "gridId": "GRD-ECM-GLOBAL-025",
  "parameter": "temperature",
  "level": {
    "type": "pressure",
    "value": 1000
  },
  "timestamp": 1683112800,
  "data": [
    [225, 226, 224, 223, ...], // Row 1 (values are scaled, e.g., 225 = 22.5°C)
    [224, 225, 225, 226, ...], // Row 2
    ...
  ],
  "encoding": "raw", // Data encoding method
  "hash": "0xae32..." // Hash of the data matrix for verification
}
```

#### 2. Sparse Grid (points of interest only)

```json
{
  "gridId": "GRD-ECM-GLOBAL-025",
  "parameter": "temperature",
  "level": {
    "type": "pressure",
    "value": 1000
  },
  "timestamp": 1683112800,
  "points": [
    {
      "lat": 40750000,
      "lon": -74000000,
      "value": 225 // 22.5°C
    },
    {
      "lat": 40750000,
      "lon": -73750000,
      "value": 226 // 22.6°C
    }
    // More points
  ],
  "encoding": "sparse",
  "hash": "0x7c12..." // Hash of all points for verification
}
```

## Weather Forecast Format

Forecast data includes predictions for future weather conditions.

### Forecast Metadata

```json
{
  "id": "FCST-NYC-48H",
  "type": "hourly", // hourly, daily, weekly
  "location": {
    "latitude": 40712800,
    "longitude": -74006000,
    "name": "New York City"
  },
  "modelName": "GFS", // Global Forecast System
  "modelVersion": "v16.2",
  "modelRunTime": 1683100800, // When the model was run
  "forecastHorizon": 48, // Hours
  "spatialResolution": 0.25, // Degrees
  "parameters": ["temperature", "precipitation", "windSpeed"]
}
```

### Forecast Data

```json
{
  "forecastId": "FCST-NYC-48H",
  "issuanceTime": 1683112800, // When the forecast was issued
  "periods": [
    {
      "startTime": 1683115200, // Start of the forecast period
      "endTime": 1683118800, // End of the forecast period
      "values": {
        "temperature": {
          "value": 225, // 22.5°C
          "min": 220, // 22.0°C
          "max": 230, // 23.0°C
          "probability": {
            "above25C": 30, // 30% probability of > 25°C
            "below20C": 10 // 10% probability of < 20°C
          }
        },
        "precipitation": {
          "value": 0, // 0.0 mm
          "probability": {
            "anyPrecipitation": 5 // 5% probability of any precipitation
          }
        },
        "windSpeed": {
          "value": 123, // 12.3 m/s
          "direction": 2700, // 270.0°
          "gust": 150 // 15.0 m/s
        }
      }
    }
    // Additional forecast periods
  ],
  "confidence": 80, // Overall forecast confidence
  "signature": "0xb743..." // Cryptographic signature verifying forecast authenticity
}
```

## Climate Data Format

Climate data represents longer-term data, typically averages over years or decades.

### Climate Dataset Metadata

```json
{
  "id": "CLIM-NYC-1990-2020",
  "type": "normals", // normals, trends, extremes
  "location": {
    "latitude": 40712800,
    "longitude": -74006000,
    "name": "New York City"
  },
  "period": {
    "start": 631152000, // Jan 1, 1990
    "end": 1609459200 // Dec 31, 2020
  },
  "parameters": ["temperature", "precipitation"],
  "resolution": "monthly", // yearly, seasonal, monthly, daily
  "source": "NOAA-NCEI",
  "version": "v2.1"
}
```

### Climate Normals Data

```json
{
  "climateId": "CLIM-NYC-1990-2020",
  "parameter": "temperature",
  "unit": "celsius",
  "data": [
    {
      "period": "January",
      "mean": 0, // 0.0°C
      "median": -1, // -0.1°C
      "min": -150, // -15.0°C (record low)
      "max": 190, // 19.0°C (record high)
      "stdDev": 25 // 2.5°C
    },
    {
      "period": "February",
      "mean": 10, // 1.0°C
      "median": 8, // 0.8°C
      "min": -120, // -12.0°C
      "max": 215, // 21.5°C
      "stdDev": 32 // 3.2°C
    }
    // Additional months
  ],
  "signature": "0xaa23..." // Cryptographic signature from climate authority
}
```

## Data Quality Indicators

All weather data includes quality indicators to help users assess reliability.

### Quality Flags

Quality flags are stored as bitfields:

| Bit | Flag              | Description                                      |
| --- | ----------------- | ------------------------------------------------ |
| 0   | Missing           | Data point is missing and was interpolated       |
| 1   | Suspicious        | Data point exceeds normal bounds                 |
| 2   | Inconsistent      | Data point inconsistent with nearby measurements |
| 3   | Unverified        | Data has not been verified by validators         |
| 4   | Instrument Error  | Sensor malfunction detected                      |
| 5   | Calibration Issue | Calibration may be out of date                   |
| 6   | Human Modified    | Data was adjusted by human intervention          |
| 7   | Reserved          | Reserved for future use                          |

### Confidence Score

A confidence score (0-100) indicates the reliability of the data:

- **95-100**: High confidence, verified by multiple validators
- **80-94**: Good confidence, passed automatic validation
- **60-79**: Moderate confidence, passed basic validation
- **40-59**: Low confidence, some quality issues present
- **0-39**: Very low confidence, significant quality issues

## Data Encoding in Transactions

Weather data is encoded in blockchain transactions using compact binary formats for efficiency.

### Binary Encoding Format

The base format for encoding weather data in transactions:

```
[4-byte version] [8-byte timestamp] [4-byte data type] [4-byte data length] [N-byte data] [32-byte signature]
```

### JSON Encoding Format

For applications that require JSON format, data can be encoded using a compact schema:

```javascript
// Example of compact JSON encoding
const compactData = {
  v: 1, // version
  ts: 1683112800, // timestamp
  dt: 1, // data type (1 = station data)
  sid: 'WS-NYC-001', // station id
  m: {
    // measurements
    t: 225, // temperature
    h: 65, // humidity
    p: 10132, // pressure
    ws: 123, // wind speed
    wd: 2700, // wind direction
    r: 0, // precipitation
  },
  q: 0, // quality flags
  c: 95, // confidence
};
```

### Compression Techniques

For larger datasets, Wrfcoin supports:

1. **Run-length encoding** for large grids
2. **Delta encoding** for time series data
3. **Dictionary compression** for repeated values
4. **Base-N encoding** for efficient string representation

## Blockchain Storage Model

Weather data is stored in the blockchain using a specialized data-aware block structure.

### Data-Aware Block Structure

Wrfcoin's data-aware blocks include:

```
[Standard Block Header]
[Transaction Merkle Root]
[Weather Data Merkle Root] ← Special structure for weather data
[Block Signature]
```

### Storage Tiers

Data is stored in different tiers depending on importance:

1. **L1 (On-chain)**: Critical data stored directly in blocks
2. **L2 (Sidechain)**: Larger datasets stored in specialized sidechains
3. **L3 (IPFS/Distributed)**: Very large datasets stored with hash references

### Data Pruning and Archiving

Over time, older data is:

1. Summarized into statistical aggregates
2. Moved to archive storage
3. Maintained by specialized storage keeper nodes

## Computational Data Blocks

For weather data that requires computational processing, Wrfcoin includes specialized computational
data blocks.

### Computational Block Format

```json
{
  "blockType": "computational",
  "version": 1,
  "timestamp": 1683112800,
  "computationType": "weatherModel",
  "inputData": {
    "type": "reference",
    "reference": "0x9a42..." // Reference to input data blocks
  },
  "computationParameters": {
    "model": "SimpleWeatherPredictor",
    "version": "1.0.2",
    "configuration": {
      "resolution": "0.25deg",
      "steps": 24,
      "physics": "simplified"
    }
  },
  "outputSchema": {
    "type": "temperature_forecast",
    "dimensions": [144, 72, 24],
    "encoding": "float16"
  },
  "computationHash": "0xbb31...", // Hash of computation settings
  "resultsHash": "0xaa22...", // Hash of computation results
  "validatorSignatures": [
    {
      "validatorId": "VAL-001",
      "signature": "0x7c31..."
    },
    {
      "validatorId": "VAL-002",
      "signature": "0x9e45..."
    }
  ]
}
```

### Computation Results

Results are stored separately from the computational block:

```json
{
  "computationBlockId": "0xf742...",
  "results": {
    "format": "compressed-grid",
    "encoding": "float16",
    "data": "0x7a9b..." // Compressed binary data
  },
  "metrics": {
    "computationTime": 342, // seconds
    "resourceUsed": "cpu-hours-3.4",
    "accuracy": 0.93
  },
  "validator": "VAL-001"
}
```

These specifications provide a comprehensive guide to the weather data formats used in the Wrfcoin
platform. Developers working with the platform should follow these specifications to ensure
compatibility with the blockchain and related services.

For specific implementation details or extensions, refer to the API documentation, SDK references,
and development tools provided by the Wrfcoin platform.
