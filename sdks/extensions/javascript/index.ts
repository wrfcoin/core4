/**
 * WRFCoin SDK Extensions
 *
 * Custom functionality and advanced features that extend the auto-generated SDKs
 */

// Weather validation extensions
export { WeatherService } from './weather-validation/WeatherService.js';

// IoT device management
export * from './iot-devices/index.js';

// Compute receipt signing compatibility
export * from './compute-receipt/index.js';

export const EXTENSIONS_VERSION = '2.0.0';
