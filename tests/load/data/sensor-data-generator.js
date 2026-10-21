/**
 * Sensor Data Generator for K6 Load Testing
 * Generates realistic weather sensor data for WRFCoin IoT testing
 */

import {
  randomIntBetween,
  randomItem,
  randomString,
} from 'https://jslib.k6.io/k6-utils/1.2.0/index.js';

// Weather station locations across the globe
export const WEATHER_LOCATIONS = [
  {
    lat: 40.7128,
    lon: -74.006,
    city: 'New York',
    timezone: 'America/New_York',
    climate: 'temperate',
  },
  {
    lat: 34.0522,
    lon: -118.2437,
    city: 'Los Angeles',
    timezone: 'America/Los_Angeles',
    climate: 'mediterranean',
  },
  {
    lat: 51.5074,
    lon: -0.1278,
    city: 'London',
    timezone: 'Europe/London',
    climate: 'temperate',
  },
  {
    lat: 35.6762,
    lon: 139.6503,
    city: 'Tokyo',
    timezone: 'Asia/Tokyo',
    climate: 'humid_subtropical',
  },
  {
    lat: -33.8688,
    lon: 151.2093,
    city: 'Sydney',
    timezone: 'Australia/Sydney',
    climate: 'temperate',
  },
  {
    lat: 55.7558,
    lon: 37.6176,
    city: 'Moscow',
    timezone: 'Europe/Moscow',
    climate: 'continental',
  },
  {
    lat: 52.52,
    lon: 13.405,
    city: 'Berlin',
    timezone: 'Europe/Berlin',
    climate: 'temperate',
  },
  {
    lat: 19.4326,
    lon: -99.1332,
    city: 'Mexico City',
    timezone: 'America/Mexico_City',
    climate: 'subtropical',
  },
  {
    lat: 1.3521,
    lon: 103.8198,
    city: 'Singapore',
    timezone: 'Asia/Singapore',
    climate: 'tropical',
  },
  {
    lat: -34.6037,
    lon: -58.3816,
    city: 'Buenos Aires',
    timezone: 'America/Argentina/Buenos_Aires',
    climate: 'temperate',
  },
  {
    lat: 25.0343,
    lon: 121.5645,
    city: 'Taipei',
    timezone: 'Asia/Taipei',
    climate: 'subtropical',
  },
  {
    lat: 59.9139,
    lon: 10.7522,
    city: 'Oslo',
    timezone: 'Europe/Oslo',
    climate: 'subarctic',
  },
  {
    lat: -26.2041,
    lon: 28.0473,
    city: 'Johannesburg',
    timezone: 'Africa/Johannesburg',
    climate: 'subtropical',
  },
  {
    lat: 13.7563,
    lon: 100.5018,
    city: 'Bangkok',
    timezone: 'Asia/Bangkok',
    climate: 'tropical',
  },
  {
    lat: 45.4642,
    lon: 9.19,
    city: 'Milan',
    timezone: 'Europe/Rome',
    climate: 'humid_subtropical',
  },
];

// Climate-specific temperature ranges (Celsius)
const CLIMATE_RANGES = {
  tropical: { temp: { min: 20, max: 35 }, humidity: { min: 70, max: 95 } },
  subtropical: { temp: { min: 10, max: 30 }, humidity: { min: 50, max: 85 } },
  temperate: { temp: { min: -5, max: 25 }, humidity: { min: 40, max: 80 } },
  continental: { temp: { min: -20, max: 30 }, humidity: { min: 30, max: 75 } },
  mediterranean: { temp: { min: 5, max: 35 }, humidity: { min: 35, max: 70 } },
  humid_subtropical: {
    temp: { min: 0, max: 35 },
    humidity: { min: 60, max: 90 },
  },
  subarctic: { temp: { min: -30, max: 15 }, humidity: { min: 50, max: 85 } },
};

// Sensor quality distributions
const SENSOR_QUALITY = {
  excellent: 0.7, // 70% of readings
  good: 0.25, // 25% of readings
  fair: 0.04, // 4% of readings
  poor: 0.01, // 1% of readings
};

// Sensor types and their specifications
const SENSOR_TYPES = {
  temperature: {
    unit: 'celsius',
    precision: 0.1,
    range: { min: -40, max: 60 },
    errorRate: 0.02,
  },
  humidity: {
    unit: 'percent',
    precision: 0.5,
    range: { min: 0, max: 100 },
    errorRate: 0.03,
  },
  pressure: {
    unit: 'hPa',
    precision: 0.1,
    range: { min: 950, max: 1050 },
    errorRate: 0.01,
  },
  windSpeed: {
    unit: 'km/h',
    precision: 0.1,
    range: { min: 0, max: 200 },
    errorRate: 0.02,
  },
  windDirection: {
    unit: 'degrees',
    precision: 1,
    range: { min: 0, max: 360 },
    errorRate: 0.01,
  },
  rainfall: {
    unit: 'mm',
    precision: 0.1,
    range: { min: 0, max: 500 },
    errorRate: 0.02,
  },
  solarRadiation: {
    unit: 'W/m²',
    precision: 1,
    range: { min: 0, max: 1400 },
    errorRate: 0.01,
  },
  uvIndex: {
    unit: 'index',
    precision: 0.1,
    range: { min: 0, max: 15 },
    errorRate: 0.01,
  },
};

/**
 * Generate a realistic weather station configuration
 */
export function generateWeatherStation(stationId, location) {
  const station = location || randomItem(WEATHER_LOCATIONS);

  return {
    id: stationId || `WS_${randomString(8).toUpperCase()}`,
    name: `Weather Station ${station.city}`,
    location: {
      latitude: station.lat + (Math.random() - 0.5) * 0.1, // Add some variance
      longitude: station.lon + (Math.random() - 0.5) * 0.1,
      altitude: randomIntBetween(0, 2000),
      city: station.city,
      timezone: station.timezone,
      climate: station.climate,
    },
    sensors: Object.keys(SENSOR_TYPES),
    firmware: `v${randomIntBetween(1, 3)}.${randomIntBetween(0, 9)}.${randomIntBetween(0, 9)}`,
    installationDate: new Date(
      Date.now() - randomIntBetween(30, 1095) * 24 * 60 * 60 * 1000
    ).toISOString(),
    lastMaintenanceDate: new Date(
      Date.now() - randomIntBetween(1, 90) * 24 * 60 * 60 * 1000
    ).toISOString(),
    owner: `user-${randomIntBetween(1, 1000)}`,
    status: randomItem(['active', 'maintenance', 'calibrating']),
    dataTransmissionInterval: randomIntBetween(300, 1800), // 5-30 minutes in seconds
  };
}

/**
 * Generate realistic sensor reading based on climate and time
 */
export function generateSensorReading(station, timestamp) {
  const location = station.location;
  const climate = CLIMATE_RANGES[location.climate] || CLIMATE_RANGES.temperate;
  const readingTime = timestamp ? new Date(timestamp) : new Date();

  // Calculate time-based variations
  const hourOfDay = readingTime.getHours();
  const dayOfYear = Math.floor(
    (readingTime.getTime() -
      new Date(readingTime.getFullYear(), 0, 0).getTime()) /
      (24 * 60 * 60 * 1000)
  );

  // Seasonal variation (simplified sine wave)
  const seasonalFactor = Math.sin((dayOfYear / 365) * 2 * Math.PI);

  // Daily variation (temperature peaks in afternoon)
  const dailyFactor = Math.sin(((hourOfDay - 6) / 24) * 2 * Math.PI);

  // Base temperature with seasonal and daily variations
  const baseTemp = (climate.temp.min + climate.temp.max) / 2;
  const tempRange = (climate.temp.max - climate.temp.min) / 2;
  const temperature =
    baseTemp +
    seasonalFactor * tempRange * 0.8 +
    dailyFactor * tempRange * 0.3 +
    randomIntBetween(-3, 3);

  // Humidity inversely correlated with temperature
  const baseHumidity = (climate.humidity.min + climate.humidity.max) / 2;
  const humidityVariation =
    (30 - temperature) * 1.5 + randomIntBetween(-15, 15);
  const humidity = Math.max(
    climate.humidity.min,
    Math.min(climate.humidity.max, baseHumidity + humidityVariation)
  );

  // Pressure with realistic variations
  const basePressure = 1013.25;
  const pressureVariation = randomIntBetween(-30, 30);
  const pressure = basePressure + pressureVariation;

  // Wind patterns (more wind during day, storms)
  const isStormyWeather = Math.random() < 0.05; // 5% chance of stormy weather
  const windSpeedBase = isStormyWeather
    ? randomIntBetween(30, 80)
    : randomIntBetween(0, 25);
  const windSpeed = Math.max(0, windSpeedBase + dailyFactor * 5);

  // Rainfall (correlated with low pressure and high humidity)
  const rainfallProbability =
    Math.max(0, (humidity - 70) / 30) * Math.max(0, (1020 - pressure) / 40);
  const rainfall =
    Math.random() < rainfallProbability ? randomIntBetween(1, 50) / 10 : 0;

  // Solar radiation (based on time of day and weather)
  const solarFactor = Math.max(0, Math.sin(((hourOfDay - 6) / 12) * Math.PI));
  const cloudiness = humidity > 80 ? 0.3 : 0.8; // Cloudy when humid
  const solarRadiation =
    solarFactor * 1200 * cloudiness * (rainfall === 0 ? 1 : 0.2);

  // UV index (correlated with solar radiation)
  const uvIndex = (solarRadiation / 1200) * 12;

  return {
    readingId: `${station.id}_${readingTime.getTime()}_${randomString(6)}`,
    stationId: station.id,
    timestamp: readingTime.toISOString(),
    location: {
      latitude: location.latitude + (Math.random() - 0.5) * 0.001, // GPS drift
      longitude: location.longitude + (Math.random() - 0.5) * 0.001,
      altitude: location.altitude + randomIntBetween(-5, 5),
    },
    sensors: {
      temperature: generateSensorValue('temperature', temperature),
      humidity: generateSensorValue('humidity', humidity),
      pressure: generateSensorValue('pressure', pressure),
      windSpeed: generateSensorValue('windSpeed', windSpeed),
      windDirection: generateSensorValue(
        'windDirection',
        randomIntBetween(0, 360)
      ),
      rainfall: generateSensorValue('rainfall', rainfall),
      solarRadiation: generateSensorValue('solarRadiation', solarRadiation),
      uvIndex: generateSensorValue('uvIndex', uvIndex),
    },
    metadata: {
      firmware: station.firmware,
      batteryLevel: randomIntBetween(20, 100),
      signalStrength: randomIntBetween(-90, -30),
      dataIntegrity: 'verified',
      calibrationStatus: randomItem(['current', 'due', 'overdue']),
      processingFlags: generateProcessingFlags(isStormyWeather, rainfall > 0),
      qualityScore: calculateQualityScore(),
    },
    environmental: {
      season: getSeason(dayOfYear),
      timeOfDay: getTimeOfDay(hourOfDay),
      weatherCondition: getWeatherCondition(
        temperature,
        humidity,
        rainfall,
        windSpeed
      ),
      climate: location.climate,
    },
  };
}

/**
 * Generate sensor value with realistic precision and quality
 */
function generateSensorValue(sensorType, baseValue) {
  const sensor = SENSOR_TYPES[sensorType];
  const quality = selectSensorQuality();

  // Add measurement error based on quality
  let error = 0;
  switch (quality) {
    case 'excellent':
      error = (Math.random() - 0.5) * sensor.precision * 0.5;
      break;
    case 'good':
      error = (Math.random() - 0.5) * sensor.precision * 2;
      break;
    case 'fair':
      error = (Math.random() - 0.5) * sensor.precision * 5;
      break;
    case 'poor':
      error = (Math.random() - 0.5) * sensor.precision * 10;
      break;
  }

  const value = baseValue + error;
  const clampedValue = Math.max(
    sensor.range.min,
    Math.min(sensor.range.max, value)
  );

  return {
    value: Math.round(clampedValue / sensor.precision) * sensor.precision,
    unit: sensor.unit,
    quality: quality,
    precision: sensor.precision,
    lastCalibration: new Date(
      Date.now() - randomIntBetween(1, 180) * 24 * 60 * 60 * 1000
    ).toISOString(),
  };
}

/**
 * Select sensor quality based on probability distribution
 */
function selectSensorQuality() {
  const random = Math.random();
  let cumulative = 0;

  for (const [quality, probability] of Object.entries(SENSOR_QUALITY)) {
    cumulative += probability;
    if (random <= cumulative) {
      return quality;
    }
  }

  return 'excellent'; // Fallback
}

/**
 * Generate processing flags based on conditions
 */
function generateProcessingFlags(isStormy, hasRainfall) {
  const flags = [];

  if (isStormy) flags.push('storm_conditions');
  if (hasRainfall) flags.push('precipitation_detected');
  if (Math.random() < 0.05) flags.push('sensor_cleaning_needed');
  if (Math.random() < 0.02) flags.push('calibration_reminder');
  if (Math.random() < 0.01) flags.push('maintenance_due');

  return flags;
}

/**
 * Calculate overall quality score
 */
function calculateQualityScore() {
  return Math.round((Math.random() * 0.3 + 0.7) * 100); // 70-100 range
}

/**
 * Get season based on day of year (Northern Hemisphere)
 */
function getSeason(dayOfYear) {
  if (dayOfYear < 80 || dayOfYear > 355) return 'winter';
  if (dayOfYear < 172) return 'spring';
  if (dayOfYear < 266) return 'summer';
  return 'autumn';
}

/**
 * Get time of day category
 */
function getTimeOfDay(hour) {
  if (hour < 6) return 'night';
  if (hour < 12) return 'morning';
  if (hour < 18) return 'afternoon';
  return 'evening';
}

/**
 * Determine weather condition based on sensor readings
 */
function getWeatherCondition(temperature, humidity, rainfall, windSpeed) {
  if (rainfall > 5) return 'heavy_rain';
  if (rainfall > 0) return 'light_rain';
  if (windSpeed > 50) return 'windy';
  if (humidity > 85) return 'humid';
  if (temperature > 30) return 'hot';
  if (temperature < 0) return 'freezing';
  if (humidity < 30) return 'dry';
  return 'clear';
}

/**
 * Generate batch of sensor readings for multiple stations
 */
export function generateBatchSensorReadings(stations, count = 100) {
  const readings = [];
  const now = Date.now();

  for (let i = 0; i < count; i++) {
    const station = randomItem(stations);
    const timestamp = now - randomIntBetween(0, 3600000); // Within last hour
    readings.push(generateSensorReading(station, timestamp));
  }

  return readings;
}

/**
 * Generate test weather stations for load testing
 */
export function generateTestWeatherStations(count = 100) {
  const stations = [];

  for (let i = 0; i < count; i++) {
    const location = randomItem(WEATHER_LOCATIONS);
    const stationId = `WS_${String(i + 1).padStart(3, '0')}`;
    stations.push(generateWeatherStation(stationId, location));
  }

  return stations;
}

export default {
  generateWeatherStation,
  generateSensorReading,
  generateBatchSensorReadings,
  generateTestWeatherStations,
  WEATHER_LOCATIONS,
  CLIMATE_RANGES,
  SENSOR_TYPES,
};
