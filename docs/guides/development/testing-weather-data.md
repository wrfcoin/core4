# Weather Data Validation Testing Guide

This guide demonstrates how to use the Wrfcoin Weather Data Validation utilities to ensure the
integrity and accuracy of meteorological data on the blockchain.

## Table of Contents

1. [Basic Data Validation](#basic-data-validation)
2. [Time Series Validation](#time-series-validation)
3. [Weather Station Data Validation](#weather-station-data-validation)
4. [Grid Data Validation](#grid-data-validation)
5. [Comparing to Reference Data](#comparing-to-reference-data)
6. [Data Integrity Checking](#data-integrity-checking)

## Basic Data Validation

Start with basic validation of individual weather data points:

```javascript
const { WeatherDataValidator } = require('../../utils/testing');

// Create a validator with custom configuration
const validator = new WeatherDataValidator({
  requiredFields: ['temperature', 'humidity', 'windSpeed', 'pressure'],
  temperatureRange: { min: -60, max: 60 }, // in Celsius
  humidityRange: { min: 0, max: 100 }, // in percent
  windSpeedRange: { min: 0, max: 150 }, // in km/h
  pressureRange: { min: 870, max: 1090 }, // in hPa
});

// Validate a single data point
const dataPoint = {
  location: 'Berlin',
  temperature: 22.5,
  humidity: 65,
  windSpeed: 12.3,
  pressure: 1013.2,
  timestamp: Date.now(),
};

const validationResult = validator.validateDataPoint(dataPoint);

if (validationResult.valid) {
  console.log('Data point is valid');
} else {
  console.log('Validation errors:', validationResult.errors);
}

// Test with an invalid data point
const invalidDataPoint = {
  location: 'Berlin',
  temperature: 120, // Far outside valid range
  humidity: 65,
  timestamp: Date.now(),
  // Missing required windSpeed and pressure
};

const invalidResult = validator.validateDataPoint(invalidDataPoint);
console.log('Invalid data point errors:', invalidResult.errors);
```

## Time Series Validation

Validate time series data and detect outliers:

```javascript
const { WeatherDataValidator } = require('../../utils/testing');

const validator = new WeatherDataValidator();

// A time series of temperature readings
const timeSeries = [
  { timestamp: Date.now() - 3600000 * 3, temperature: 22.1, humidity: 60 },
  { timestamp: Date.now() - 3600000 * 2, temperature: 22.5, humidity: 62 },
  { timestamp: Date.now() - 3600000 * 1, temperature: 22.8, humidity: 63 },
  { timestamp: Date.now(), temperature: 45.2, humidity: 30 }, // Potential outlier
];

// Validate the time series
const timeSeriesValidation = validator.validateTimeSeries(timeSeries);
console.log('Time series validation:', timeSeriesValidation);

// Detect outliers in temperature data
const temperatureOutliers = validator.detectOutliers(timeSeries, 'temperature', 2.0);
console.log('Temperature outliers:', temperatureOutliers);

// Check variable consistency (temperature vs humidity)
const consistencyIssues = validator.checkVariableConsistency(timeSeries);
console.log('Consistency issues:', consistencyIssues);

// Calculate temporal consistency (how smoothly temperature changes over time)
const tempConsistency = validator.calculateTemporalConsistency(timeSeries, 'temperature');
console.log('Temperature temporal consistency:', tempConsistency);

// If we found outliers, we might want to clean the data
if (temperatureOutliers.length > 0) {
  const cleanedData = timeSeries.filter(
    (_, index) => !temperatureOutliers.some(outlier => outlier.index === index)
  );
  console.log('Cleaned data:', cleanedData);
}
```

## Weather Station Data Validation

Validate data from weather stations with their specific metadata:

```javascript
const { StationDataValidator } = require('../../utils/testing');

// Create a station validator with custom rules for stations
const stationValidator = new StationDataValidator({
  stationTypes: ['official', 'amateur', 'research'],
  requiredMetadataFields: ['id', 'name', 'location', 'elevation', 'type'],
});

// Station metadata
const stationMetadata = {
  id: 'STATION-123',
  name: 'Berlin Central',
  location: { lat: 52.52, lon: 13.4 },
  elevation: 74, // meters above sea level
  type: 'official',
  instruments: ['thermometer', 'hygrometer', 'barometer', 'anemometer'],
  reportingInterval: 300, // seconds
};

// Validate station metadata
const metadataResult = stationValidator.validateStationMetadata(stationMetadata);
console.log('Station metadata validation:', metadataResult);

// Station data records
const stationData = [
  {
    stationId: 'STATION-123',
    timestamp: Date.now() - 600000,
    temperature: 22.5,
    humidity: 65,
    pressure: 1013.2,
    windSpeed: 12.3,
    windDirection: 270,
  },
  {
    stationId: 'STATION-123',
    timestamp: Date.now() - 300000,
    temperature: 22.7,
    humidity: 64,
    pressure: 1013.0,
    windSpeed: 11.9,
    windDirection: 275,
  },
  {
    stationId: 'STATION-123',
    timestamp: Date.now(),
    temperature: 22.9,
    humidity: 64,
    pressure: 1012.8,
    windSpeed: 12.1,
    windDirection: 280,
  },
];

// Validate station data against metadata
const dataValidation = stationValidator.validateAgainstMetadata(stationData, stationMetadata);
console.log('Station data validation:', dataValidation);

// Check for data collection gaps based on reportingInterval
const expectedRecords = 3; // Based on time range and reporting interval
if (stationData.length < expectedRecords) {
  console.log(
    `Warning: Missing data. Expected ${expectedRecords} records but found ${stationData.length}`
  );
}
```

## Grid Data Validation

Validate gridded weather products like numerical weather prediction models:

```javascript
const { GridDataValidator } = require('../../utils/testing');

// Create a grid validator
const gridValidator = new GridDataValidator({
  gridResolutions: [0.25, 0.5, 1.0], // in degrees
  supportedProjections: ['WGS84', 'Lambert', 'Mercator'],
  temporalResolutions: [1, 3, 6, 12, 24], // hours
});

// Grid metadata
const gridMetadata = {
  model: 'GFS',
  resolution: 0.5, // degrees
  projection: 'WGS84',
  temporalResolution: 6, // hours
  forecastHorizon: 72, // hours
  variables: ['temperature', 'precipitation', 'windSpeed', 'windDirection'],
};

// Validate grid metadata
const gridMetadataResult = gridValidator.validateGridMetadata(gridMetadata);
console.log('Grid metadata validation:', gridMetadataResult);

// Sample grid points (simplified for example)
const gridPoints = [
  { lat: 52.0, lon: 13.0, temperature: 22.5, precipitation: 0 },
  { lat: 52.0, lon: 13.5, temperature: 22.7, precipitation: 0 },
  { lat: 52.5, lon: 13.0, temperature: 22.3, precipitation: 0 },
  { lat: 52.5, lon: 13.5, temperature: 22.6, precipitation: 0 },
  { lat: 52.5, lon: 14.0, temperature: 35.2, precipitation: 0 }, // Potential spatial inconsistency
];

// Validate grid points
const gridPointsResult = gridValidator.validateGridPoints(gridPoints);
console.log('Grid points validation:', gridPointsResult);

// Check spatial consistency of the grid
const spatialIssues = gridValidator.checkSpatialConsistency(gridPoints);
console.log('Spatial consistency issues:', spatialIssues);

// If we found issues, we could interpolate or correct the problematic value
if (spatialIssues.length > 0) {
  console.log(
    'Problematic grid points that need correction:',
    spatialIssues.map(issue => gridPoints[issue.index])
  );
}
```

## Comparing to Reference Data

Compare your weather data against trusted reference sources:

```javascript
const { ReferenceDataComparator } = require('../../utils/testing');

// Create a comparator with tolerance settings
const comparator = new ReferenceDataComparator({
  maxTemperatureDelta: 1.5, // °C
  maxHumidityDelta: 5, // %
  maxPressureDelta: 2, // hPa
  maxDistanceForComparison: 5, // km
  maxTimeOffsetForComparison: 3600000, // ms (1 hour)
});

// Your test data
const testData = [
  {
    location: { lat: 52.52, lon: 13.4 },
    timestamp: Date.now(),
    temperature: 22.5,
    humidity: 65,
    pressure: 1013.2,
  },
];

// Reference data from a trusted source
const referenceData = [
  {
    location: { lat: 52.51, lon: 13.41 }, // Slightly different location
    timestamp: Date.now() - 1800000, // 30 minutes earlier
    temperature: 22.2,
    humidity: 63,
    pressure: 1012.9,
  },
];

// Compare the data
const comparisonResult = comparator.compareData(testData, referenceData);
console.log('Comparison with reference data:', comparisonResult);

// Detailed comparison for the first data point
if (comparisonResult.points.length > 0) {
  const pointComparison = comparisonResult.points[0];

  console.log('Temperature difference:', pointComparison.temperature.difference);
  console.log('Within acceptable range:', pointComparison.temperature.withinTolerance);

  console.log('Humidity difference:', pointComparison.humidity.difference);
  console.log('Within acceptable range:', pointComparison.humidity.withinTolerance);

  console.log('Pressure difference:', pointComparison.pressure.difference);
  console.log('Within acceptable range:', pointComparison.pressure.withinTolerance);

  console.log('Overall match quality:', pointComparison.overallQuality);
}
```

## Data Integrity Checking

Verify the integrity of weather data through hashing and segmentation:

```javascript
const { DataIntegrityChecker } = require('../../utils/testing');

// Create an integrity checker
const integrityChecker = new DataIntegrityChecker();

// Sample weather data
const weatherData = [
  { location: 'Berlin', temperature: 22.5, timestamp: Date.now() - 3600000 * 2 },
  { location: 'Berlin', temperature: 22.8, timestamp: Date.now() - 3600000 * 1 },
  { location: 'Berlin', temperature: 23.1, timestamp: Date.now() },
];

// Generate a hash for the data set
const dataHash = integrityChecker.computeDataHash(weatherData);
console.log('Data hash:', dataHash);

// Store the hash on the blockchain for future verification
// (simulated here)
const storedHash = dataHash;

// Later, verify the data hasn't been tampered with
const verificationResult = integrityChecker.verifyIntegrity(weatherData, storedHash);
console.log('Verification result:', verificationResult);

// For larger datasets, generate a report with segment hashes
const integrityReport = integrityChecker.generateIntegrityReport(weatherData, 2);
console.log('Integrity report with segments:', integrityReport);

// Save this report for future verification of data segments
```

These examples demonstrate how to use the weather data validation utilities in the Wrfcoin Testing
Framework. You can adapt these patterns to ensure the accuracy, consistency, and integrity of
weather data in your blockchain applications.
