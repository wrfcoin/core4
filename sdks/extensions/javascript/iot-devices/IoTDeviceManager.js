/**
 * IoTDeviceManager
 *
 * This module provides a central manager for handling multiple IoT devices
 * connected to the Wrfcoin network. It allows for managing devices of different
 * types, handling data from multiple sources, and aggregating analytics.
 */

import WeatherStationClient from './WeatherStationClient.js';
import AirQualityMonitorClient from './AirQualityMonitorClient.js';

class IoTDeviceManager {
  /**
   * Create a new IoTDeviceManager
   *
   * @param {Object} options - Configuration options
   * @param {Object} options.sdk - Wrfcoin SDK instance
   * @param {string} [options.network='testnet'] - Network to use
   */
  constructor(options) {
    if (!options || !options.sdk) {
      throw new Error('Wrfcoin SDK instance is required');
    }

    this.sdk = options.sdk;
    this.network = options.network || 'testnet';
    this.devices = new Map();
    this.eventListeners = {
      'device-added': [],
      'device-removed': [],
      'data-received': [],
      error: [],
      alert: [],
    };
  }

  /**
   * Add a weather station to the manager
   *
   * @param {string} deviceId - Device ID or a custom identifier
   * @param {Object} deviceInfo - Device information
   * @param {Object} [options] - Additional options
   * @returns {WeatherStationClient} - The created weather station client
   */
  addWeatherStation(deviceId, deviceInfo, options = {}) {
    if (this.devices.has(deviceId)) {
      throw new Error(`Device with ID ${deviceId} already exists`);
    }

    // Create weather station client
    const client = new WeatherStationClient({
      sdk: this.sdk,
      deviceInfo,
      network: this.network,
      ...options,
    });

    // Setup event forwarding
    this._setupEventForwarding(client, deviceId);

    // Store device
    this.devices.set(deviceId, {
      id: deviceId,
      type: 'WEATHER_STATION',
      client,
      info: deviceInfo,
      status: 'inactive',
    });

    // Emit device added event
    this._emitEvent('device-added', {
      deviceId,
      type: 'WEATHER_STATION',
      info: deviceInfo,
    });

    return client;
  }

  /**
   * Add an air quality monitor to the manager
   *
   * @param {string} deviceId - Device ID or a custom identifier
   * @param {Object} deviceInfo - Device information
   * @param {Object} [options] - Additional options
   * @returns {AirQualityMonitorClient} - The created air quality monitor client
   */
  addAirQualityMonitor(deviceId, deviceInfo, options = {}) {
    if (this.devices.has(deviceId)) {
      throw new Error(`Device with ID ${deviceId} already exists`);
    }

    // Create air quality monitor client
    const client = new AirQualityMonitorClient({
      sdk: this.sdk,
      deviceInfo,
      network: this.network,
      ...options,
    });

    // Setup event forwarding
    this._setupEventForwarding(client, deviceId);

    // Store device
    this.devices.set(deviceId, {
      id: deviceId,
      type: 'AIR_QUALITY_MONITOR',
      client,
      info: deviceInfo,
      status: 'inactive',
    });

    // Emit device added event
    this._emitEvent('device-added', {
      deviceId,
      type: 'AIR_QUALITY_MONITOR',
      info: deviceInfo,
    });

    return client;
  }

  /**
   * Get a device by ID
   *
   * @param {string} deviceId - Device ID
   * @returns {Object|null} - Device object or null if not found
   */
  getDevice(deviceId) {
    return this.devices.get(deviceId) || null;
  }

  /**
   * Get all devices
   *
   * @returns {Array} - Array of device objects
   */
  getAllDevices() {
    return Array.from(this.devices.values());
  }

  /**
   * Get all devices of a specific type
   *
   * @param {string} type - Device type
   * @returns {Array} - Array of device objects
   */
  getDevicesByType(type) {
    return Array.from(this.devices.values()).filter(
      device => device.type === type
    );
  }

  /**
   * Remove a device from the manager
   *
   * @param {string} deviceId - Device ID
   * @returns {boolean} - True if device was removed, false otherwise
   */
  removeDevice(deviceId) {
    const device = this.devices.get(deviceId);
    if (!device) {
      return false;
    }

    // Stop the device if it's running
    if (device.client) {
      try {
        device.client.stop();
      } catch (error) {
        console.error(`Error stopping device ${deviceId}:`, error);
      }
    }

    // Remove the device
    this.devices.delete(deviceId);

    // Emit device removed event
    this._emitEvent('device-removed', {
      deviceId,
      type: device.type,
    });

    return true;
  }

  /**
   * Start all devices
   *
   * @returns {Promise<Object>} - Results of starting all devices
   */
  async startAll() {
    const results = {
      success: [],
      failed: [],
    };

    for (const [deviceId, device] of this.devices.entries()) {
      try {
        if (device.client) {
          await device.client.start();
          device.status = 'active';
          results.success.push(deviceId);
        }
      } catch (error) {
        results.failed.push({
          deviceId,
          error: error.message,
        });
        this._emitEvent('error', {
          deviceId,
          operation: 'start',
          message: error.message,
          error,
        });
      }
    }

    return results;
  }

  /**
   * Stop all devices
   */
  stopAll() {
    for (const [deviceId, device] of this.devices.entries()) {
      try {
        if (device.client) {
          device.client.stop();
          device.status = 'inactive';
        }
      } catch (error) {
        this._emitEvent('error', {
          deviceId,
          operation: 'stop',
          message: error.message,
          error,
        });
      }
    }
  }

  /**
   * Fetch data from a specific device
   *
   * @param {string} deviceId - Device ID
   * @returns {Promise<Object>} - Device data
   */
  async fetchData(deviceId) {
    const device = this.devices.get(deviceId);
    if (!device || !device.client) {
      throw new Error(`Device ${deviceId} not found`);
    }

    return await device.client.readAndSubmitData();
  }

  /**
   * Fetch data from all devices
   *
   * @returns {Promise<Object>} - Results of fetching data from all devices
   */
  async fetchAllData() {
    const results = {
      success: [],
      failed: [],
    };

    for (const [deviceId, device] of this.devices.entries()) {
      try {
        if (device.client) {
          const data = await device.client.readAndSubmitData();
          results.success.push({
            deviceId,
            data,
          });
        }
      } catch (error) {
        results.failed.push({
          deviceId,
          error: error.message,
        });
        this._emitEvent('error', {
          deviceId,
          operation: 'fetch-data',
          message: error.message,
          error,
        });
      }
    }

    return results;
  }

  /**
   * Process offline queues for all devices
   *
   * @returns {Promise<Object>} - Results of processing offline queues
   */
  async processAllOfflineQueues() {
    const results = {
      success: [],
      failed: [],
    };

    for (const [deviceId, device] of this.devices.entries()) {
      try {
        if (
          device.client &&
          typeof device.client.processOfflineQueue === 'function'
        ) {
          const result = await device.client.processOfflineQueue();
          results.success.push({
            deviceId,
            result,
          });
        }
      } catch (error) {
        results.failed.push({
          deviceId,
          error: error.message,
        });
        this._emitEvent('error', {
          deviceId,
          operation: 'process-offline-queue',
          message: error.message,
          error,
        });
      }
    }

    return results;
  }

  /**
   * Get aggregated statistics for all devices of a specific type
   *
   * @param {string} type - Device type
   * @param {string} parameter - Parameter to aggregate
   * @returns {Object} - Aggregated statistics
   */
  getAggregatedStats(type, parameter) {
    const devices = this.getDevicesByType(type);
    const values = [];

    for (const device of devices) {
      // Assuming the most recent data is available on the device object
      if (device.lastData && device.lastData[parameter] !== undefined) {
        values.push(device.lastData[parameter]);
      }
    }

    if (values.length === 0) {
      return {
        count: 0,
        min: null,
        max: null,
        avg: null,
        median: null,
      };
    }

    // Sort values for min, max, and median
    values.sort((a, b) => a - b);

    // Calculate statistics
    const min = values[0];
    const max = values[values.length - 1];
    const sum = values.reduce((acc, val) => acc + val, 0);
    const avg = sum / values.length;

    // Calculate median
    let median;
    const mid = Math.floor(values.length / 2);
    if (values.length % 2 === 0) {
      median = (values[mid - 1] + values[mid]) / 2;
    } else {
      median = values[mid];
    }

    return {
      count: values.length,
      min,
      max,
      avg,
      median,
    };
  }

  /**
   * Add an event listener
   *
   * @param {string} event - Event name
   * @param {Function} callback - Event callback
   */
  on(event, callback) {
    if (this.eventListeners[event]) {
      this.eventListeners[event].push(callback);
    } else {
      throw new Error(`Unknown event: ${event}`);
    }
  }

  /**
   * Remove an event listener
   *
   * @param {string} event - Event name
   * @param {Function} callback - Event callback
   */
  off(event, callback) {
    if (this.eventListeners[event]) {
      this.eventListeners[event] = this.eventListeners[event].filter(
        listener => listener !== callback
      );
    }
  }

  /**
   * Setup event forwarding from a device client
   *
   * @param {Object} client - Device client
   * @param {string} deviceId - Device ID
   * @private
   */
  _setupEventForwarding(client, deviceId) {
    // Forward data-submitted events
    client.on('data-submitted', data => {
      const device = this.devices.get(deviceId);
      if (device) {
        // Store last data on device
        device.lastData = data.data;
      }

      this._emitEvent('data-received', {
        deviceId,
        ...data,
      });
    });

    // Forward error events
    client.on('error', error => {
      this._emitEvent('error', {
        deviceId,
        ...error,
      });
    });

    // Forward status-changed events
    client.on('status-changed', statusData => {
      const device = this.devices.get(deviceId);
      if (device) {
        device.status = statusData.status;
      }
    });

    // Forward alert events (for air quality monitors)
    if (
      client.on &&
      typeof client.on === 'function' &&
      client.eventListeners &&
      client.eventListeners['alert-triggered']
    ) {
      client.on('alert-triggered', alert => {
        this._emitEvent('alert', {
          deviceId,
          ...alert,
        });
      });
    }
  }

  /**
   * Emit an event
   *
   * @param {string} event - Event name
   * @param {Object} data - Event data
   * @private
   */
  _emitEvent(event, data) {
    if (this.eventListeners[event]) {
      for (const callback of this.eventListeners[event]) {
        try {
          callback(data);
        } catch (error) {
          console.error(`Error in ${event} event handler:`, error);
        }
      }
    }
  }
}

export { IoTDeviceManager };
export default IoTDeviceManager;
