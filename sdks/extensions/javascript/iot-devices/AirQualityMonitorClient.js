/**
 * AirQualityMonitorClient
 *
 * This module provides a client for connecting air quality monitors to the Wrfcoin network.
 * It handles device registration, data submission, and status updates.
 */

function sanitizeStatusText(value, maxLength, label) {
  if (typeof value !== 'string' || value.length > maxLength) {
    throw new Error(`Physical device adapter returned invalid ${label}`);
  }
  const sanitized = value
    .replace(/[\u0000-\u001f\u007f]+/g, ' ')
    .replace(/\s+/g, ' ')
    .trim();
  if (!sanitized) {
    throw new Error(`Physical device adapter returned invalid ${label}`);
  }
  return sanitized;
}

class AirQualityMonitorClient {
  /**
   * Create a new AirQualityMonitorClient
   *
   * @param {Object} options - Configuration options
   * @param {Object} options.sdk - Wrfcoin SDK instance
   * @param {Object} options.adapter - Physical air-quality monitor adapter
   * @param {Function} options.adapter.readAirQualityData - Read a physical observation
   * @param {Object} [options.deviceInfo] - Device information
   * @param {string} [options.deviceInfo.name] - Device name
   * @param {string} [options.deviceInfo.model] - Device model
   * @param {string} [options.deviceInfo.firmwareVersion] - Device firmware version
   * @param {Object} [options.deviceInfo.location] - Device location
   * @param {number} [options.deviceInfo.location.latitude] - Location latitude
   * @param {number} [options.deviceInfo.location.longitude] - Location longitude
   * @param {number} [options.deviceInfo.location.elevation] - Location elevation in meters
   * @param {string[]} [options.deviceInfo.pollutants] - Pollutants this device can measure
   * @param {number} [options.updateInterval=10] - Data update interval in minutes
   * @param {string} [options.network='testnet'] - Network to use
   * @param {number} [options.maxQueueSize=100] - Maximum offline submissions retained
   * @param {boolean} [options.autoRetry=true] - Retry queued submissions automatically
   * @param {number} [options.retryDelay=60000] - Retry delay in milliseconds
   */
  constructor(options) {
    if (!options || !options.sdk) {
      throw new Error('Wrfcoin SDK instance is required');
    }
    if (
      !options.adapter ||
      typeof options.adapter.readAirQualityData !== 'function'
    ) {
      throw new Error(
        'A physical device adapter with readAirQualityData() is required'
      );
    }

    this.sdk = options.sdk;
    this.adapter = options.adapter;
    this.deviceInfo = options.deviceInfo || {};
    this.deviceId = null;
    this.registered = false;
    this.updateInterval = options.updateInterval || 10;
    this.network = options.network || 'testnet';
    this.updateIntervalId = null;
    this.statusCheckIntervalId = null;
    this.offlineQueue = [];
    this.maxQueueSize = options.maxQueueSize || 100;
    this.autoRetry = options.autoRetry !== false;
    this.retryDelay = options.retryDelay || 60000; // 1 minute
    this.eventListeners = {
      'data-submitted': [],
      'device-registered': [],
      error: [],
      'status-changed': [],
      'queue-full': [],
      'alert-triggered': [],
    };

    // Air quality index thresholds
    this.aqiThresholds = {
      good: 50,
      moderate: 100,
      unhealthyForSensitive: 150,
      unhealthy: 200,
      veryUnhealthy: 300,
      hazardous: 500,
    };

    // Current air quality alerts
    this.activeAlerts = [];
  }

  /**
   * Register the device with the Wrfcoin network
   *
   * @returns {Promise<Object>} - Registered device information
   */
  async register() {
    try {
      if (!this.deviceInfo.name) {
        throw new Error('Device name is required');
      }

      // If already registered with ID, skip registration
      if (this.registered && this.deviceId) {
        console.log(`Device already registered with ID: ${this.deviceId}`);
        return { id: this.deviceId, ...this.deviceInfo };
      }

      // Prepare device data for registration
      const deviceData = {
        name: this.deviceInfo.name,
        deviceType: 'AIR_QUALITY_MONITOR',
        model: this.deviceInfo.model,
        firmwareVersion: this.deviceInfo.firmwareVersion,
        location: this.deviceInfo.location,
        capabilities: {
          pollutants: this.deviceInfo.pollutants || ['pm25', 'pm10', 'o3'],
          updateInterval: this.updateInterval,
        },
        network: this.network,
      };

      // Register the device
      const result = await this.sdk.registerIoTDevice(deviceData);

      if (result && result.id) {
        this.deviceId = result.id;
        this.registered = true;
        this._emitEvent('device-registered', result);
        console.log(`Device registered successfully with ID: ${result.id}`);
        return result;
      } else {
        throw new Error('Device registration failed: No device ID returned');
      }
    } catch (error) {
      this._emitEvent('error', {
        operation: 'register',
        message: error.message,
        error,
      });
      throw error;
    }
  }

  /**
   * Start automatic data submission based on updateInterval
   *
   * @returns {Promise<void>}
   */
  async start() {
    // Make sure the device is registered
    if (!this.registered) {
      await this.register();
    }

    // Start the data submission interval
    if (!this.updateIntervalId) {
      console.log(
        `Starting automatic data submission every ${this.updateInterval} minutes`
      );
      this.updateIntervalId = setInterval(
        () => {
          this.readAndSubmitData().catch(error => {
            this._emitEvent('error', {
              operation: 'automatic-submission',
              message: error.message,
              error,
            });
          });
        },
        this.updateInterval * 60 * 1000
      );

      // Immediately submit data for the first time
      await this.readAndSubmitData();
    }

    // Start status check interval
    if (
      !this.statusCheckIntervalId &&
      typeof this.adapter.readStatus === 'function' &&
      typeof this.sdk.updateDeviceStatus === 'function'
    ) {
      this.statusCheckIntervalId = setInterval(
        () => {
          this.checkStatus().catch(error => {
            this._emitEvent('error', {
              operation: 'status-check',
              message: error.message,
              error,
            });
          });
        },
        15 * 60 * 1000
      ); // Every 15 minutes
    }
  }

  /**
   * Stop automatic data submission
   */
  stop() {
    if (this.updateIntervalId) {
      clearInterval(this.updateIntervalId);
      this.updateIntervalId = null;
      console.log('Stopped automatic data submission');
    }

    if (this.statusCheckIntervalId) {
      clearInterval(this.statusCheckIntervalId);
      this.statusCheckIntervalId = null;
    }
  }

  /**
   * Read data from the device and submit it to the network
   *
   * @returns {Promise<Object>} - Submission result
   */
  async readAndSubmitData() {
    let data;
    try {
      // Read data from the device
      data = await this._readDeviceData();

      // Check for air quality alerts
      this._checkAirQualityAlerts(data);

      // Submit data to the network
      const result = await this.submitData(data);
      return result;
    } catch (error) {
      this._emitEvent('error', {
        operation: 'read-and-submit',
        message: error.message,
        error,
      });
      throw error;
    }
  }

  /**
   * Submit data to the Wrfcoin network
   *
   * @param {Object} data - The data to submit
   * @returns {Promise<Object>} - Submission result
   */
  async submitData(data) {
    try {
      if (!this.registered || !this.deviceId) {
        throw new Error('Device not registered');
      }

      if (!data) {
        throw new Error('No data to submit');
      }

      // The observation time is data: a reading without one is refused, never stamped with "now".
      const observedAt = data.timestamp;
      const timestampValid = this._isObservationTime(observedAt);
      if (!timestampValid) {
        const missing = new Error(
          'Air-quality reading must carry an observation timestamp; none is invented'
        );
        missing.nonRetryable = true;
        throw missing;
      }

      // Prepare submission payload
      const payload = {
        deviceId: this.deviceId,
        data,
        timestamp: observedAt,
        network: this.network,
      };

      // Submit data to the network
      const result = await this.sdk.submitDeviceData(payload);

      this._emitEvent('data-submitted', {
        data,
        result,
      });

      console.log(`Data submitted successfully`);
      return result;
    } catch (error) {
      if (this.autoRetry && !error.nonRetryable) {
        this._addToOfflineQueue({
          type: 'data',
          data,
          timestamp: new Date().toISOString(),
        });
      }

      this._emitEvent('error', {
        operation: 'submit-data',
        message: error.message,
        error,
      });
      throw error;
    }
  }

  /**
   * Update device status
   *
   * @param {string} status - The status to update (active, inactive, maintenance, error)
   * @param {string} [message] - Optional message with status details
   * @returns {Promise<Object>} - Update result
   */
  async updateStatus(status, message = '') {
    try {
      if (!this.registered || !this.deviceId) {
        throw new Error('Device not registered');
      }

      const validStatuses = ['active', 'inactive', 'maintenance', 'error'];
      if (!validStatuses.includes(status.toLowerCase())) {
        throw new Error(
          `Invalid status: ${status}. Must be one of: ${validStatuses.join(', ')}`
        );
      }

      // Prepare status update payload
      const payload = {
        deviceId: this.deviceId,
        status: status.toLowerCase(),
        message,
        timestamp: new Date().toISOString(),
        network: this.network,
      };

      // Update device status
      const result = await this.sdk.updateDeviceStatus(payload);

      this._emitEvent('status-changed', {
        status: status.toLowerCase(),
        message,
        result,
      });

      console.log(`Status updated to: ${status}`);
      return result;
    } catch (error) {
      if (this.autoRetry) {
        this._addToOfflineQueue({
          type: 'status',
          status,
          message,
          timestamp: new Date().toISOString(),
        });
      }

      this._emitEvent('error', {
        operation: 'update-status',
        message: error.message,
        error,
      });
      throw error;
    }
  }

  /**
   * Check and report the device's current status
   *
   * @returns {Promise<Object>} - Status result
   */
  async checkStatus() {
    try {
      if (typeof this.adapter.readStatus !== 'function') {
        throw new Error('Physical device adapter does not implement readStatus()');
      }
      const deviceStatus = await this.adapter.readStatus();
      if (!deviceStatus || typeof deviceStatus !== 'object') {
        throw new Error('Physical device adapter returned invalid status');
      }
      const { isOnline, calibrationStatus, sensorStatus } = deviceStatus;
      if (
        typeof isOnline !== 'boolean' ||
        !calibrationStatus ||
        typeof calibrationStatus.isCalibrated !== 'boolean' ||
        !sensorStatus ||
        typeof sensorStatus.allOperational !== 'boolean' ||
        (!calibrationStatus.isCalibrated &&
          (typeof calibrationStatus.message !== 'string' ||
            calibrationStatus.message.trim() === '' ||
            calibrationStatus.message.length > 1024)) ||
        (!sensorStatus.allOperational &&
          (!Array.isArray(sensorStatus.issues) ||
            sensorStatus.issues.length === 0 ||
            sensorStatus.issues.length > 64 ||
            sensorStatus.issues.some(
              issue =>
                typeof issue !== 'string' ||
                issue.trim() === '' ||
                issue.length > 256
            )))
      ) {
        throw new Error('Physical device adapter returned incomplete status');
      }

      let status = 'active';
      let message = '';

      // Determine overall status based on checks
      if (!isOnline) {
        status = 'inactive';
        message = 'Device is offline';
      } else if (!calibrationStatus.isCalibrated) {
        status = 'maintenance';
        message = `Calibration needed: ${sanitizeStatusText(
          calibrationStatus.message,
          1024,
          'calibration message'
        )}`;
      } else if (!sensorStatus.allOperational) {
        status = 'maintenance';
        const issues = sensorStatus.issues.map(issue =>
          sanitizeStatusText(issue, 256, 'sensor issue')
        );
        message = `Sensor issues: ${issues.join(', ')}`;
      }

      // Update status
      return await this.updateStatus(status, message);
    } catch (error) {
      this._emitEvent('error', {
        operation: 'check-status',
        message: error.message,
        error,
      });
      throw error;
    }
  }

  /**
   * Calibrate the air quality sensors
   *
   * @returns {Promise<Object>} - Calibration result
   */
  async calibrateSensors() {
    try {
      if (!this.registered || !this.deviceId) {
        throw new Error('Device not registered');
      }

      // Set device to maintenance mode
      await this.updateStatus('maintenance', 'Calibrating sensors');

      // Implement actual calibration logic here
      const calibrationResult = await this._performCalibration();

      if (
        !calibrationResult ||
        typeof calibrationResult !== 'object' ||
        Array.isArray(calibrationResult) ||
        typeof calibrationResult.success !== 'boolean'
      ) {
        throw new Error('Physical device adapter returned an invalid calibration result');
      }
      if (
        calibrationResult.calibratedSensors !== undefined &&
        (!Array.isArray(calibrationResult.calibratedSensors) ||
          calibrationResult.calibratedSensors.length > 256 ||
          calibrationResult.calibratedSensors.some(
            sensor =>
              typeof sensor !== 'string' ||
              sensor.trim() === '' ||
              sensor.length > 128
          ))
      ) {
        throw new Error('Physical device adapter returned invalid calibrated sensors');
      }
      if (
        !calibrationResult.success &&
        (typeof calibrationResult.error !== 'string' ||
          calibrationResult.error.trim() === '' ||
          calibrationResult.error.length > 1024)
      ) {
        throw new Error('Physical device adapter returned failure without a valid error');
      }

      if (calibrationResult.success) {
        // Set device back to active
        await this.updateStatus('active', 'Sensors calibrated successfully');
      } else {
        // Set device to error state
        await this.updateStatus(
          'error',
          `Calibration failed: ${calibrationResult.error}`
        );
      }

      return calibrationResult;
    } catch (error) {
      // Set device to error state
      await this.updateStatus(
        'error',
        `Calibration failed: ${error.message}`
      ).catch(() => {});

      this._emitEvent('error', {
        operation: 'calibrate-sensors',
        message: error.message,
        error,
      });
      throw error;
    }
  }

  /**
   * Submit an air quality alert to the network
   *
   * @param {Object} alert - Alert information
   * @param {string} alert.pollutant - The pollutant causing the alert
   * @param {number} alert.value - The measured value
   * @param {string} alert.level - Alert level (good, moderate, unhealthyForSensitive, unhealthy, veryUnhealthy, hazardous)
   * @param {string} alert.timestamp - The timestamp of the alert
   * @returns {Promise<Object>} - Alert submission result
   */
  async submitAlert(alert) {
    try {
      if (!this.registered || !this.deviceId) {
        throw new Error('Device not registered');
      }

      if (!alert || !alert.pollutant || !alert.value || !alert.level) {
        throw new Error('Invalid alert data');
      }

      // The alert's observation time is data: refuse a missing one, never stamp "now".
      if (!this._isObservationTime(alert.timestamp)) {
        const missing = new Error(
          'Air-quality alert must carry an observation timestamp; none is invented'
        );
        missing.nonRetryable = true;
        throw missing;
      }

      // Prepare alert payload
      const payload = {
        deviceId: this.deviceId,
        type: 'AIR_QUALITY_ALERT',
        alert: {
          ...alert,
          timestamp: alert.timestamp,
        },
        location: this.deviceInfo.location,
        network: this.network,
      };

      // Submit alert to the network
      const result = await this.sdk.submitAlert(payload);

      this._emitEvent('alert-triggered', {
        alert,
        result,
      });

      console.log(
        `Air quality alert submitted: ${alert.level} level for ${alert.pollutant}`
      );
      return result;
    } catch (error) {
      this._emitEvent('error', {
        operation: 'submit-alert',
        message: error.message,
        error,
      });
      throw error;
    }
  }

  /**
   * True when `value` is a usable observation time (parseable ISO string or positive epoch number).
   *
   * @param {*} value
   * @returns {boolean}
   * @private
   */
  _isObservationTime(value) {
    return (
      (typeof value === 'string' &&
        value.trim() !== '' &&
        !Number.isNaN(Date.parse(value))) ||
      (typeof value === 'number' && Number.isFinite(value) && value > 0)
    );
  }

  /**
   * Process offline queue
   *
   * @returns {Promise<Object>} - Processing result
   */
  async processOfflineQueue() {
    if (this.offlineQueue.length === 0) {
      return { processed: 0, success: 0, failed: 0 };
    }

    console.log(`Processing offline queue: ${this.offlineQueue.length} items`);

    const results = {
      processed: 0,
      success: 0,
      failed: 0,
      errors: [],
    };

    // Process queue items
    const queue = [...this.offlineQueue];
    this.offlineQueue = [];

    for (const item of queue) {
      try {
        results.processed++;

        if (item.type === 'data') {
          await this.submitData(item.data);
          results.success++;
        } else if (item.type === 'status') {
          await this.updateStatus(item.status, item.message);
          results.success++;
        } else if (item.type === 'alert') {
          await this.submitAlert(item.alert);
          results.success++;
        }
      } catch (error) {
        results.failed++;
        results.errors.push({
          item,
          error: error.message,
        });

        // A nonRetryable refusal is permanent invalid input: leave the queue instead of looping.
        if (!error.nonRetryable) {
          this.offlineQueue.push(item);
        }
      }
    }

    return results;
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
   * Calculate Air Quality Index (AQI) for PM2.5
   *
   * @param {number} pm25 - PM2.5 concentration in µg/m³
   * @returns {number} - AQI value
   */
  calculatePM25AQI(pm25) {
    // Based on EPA AQI calculation for PM2.5
    if (pm25 <= 12.0) {
      return this._linearScale(pm25, 0, 12.0, 0, 50);
    } else if (pm25 <= 35.4) {
      return this._linearScale(pm25, 12.1, 35.4, 51, 100);
    } else if (pm25 <= 55.4) {
      return this._linearScale(pm25, 35.5, 55.4, 101, 150);
    } else if (pm25 <= 150.4) {
      return this._linearScale(pm25, 55.5, 150.4, 151, 200);
    } else if (pm25 <= 250.4) {
      return this._linearScale(pm25, 150.5, 250.4, 201, 300);
    } else if (pm25 <= 500.4) {
      return this._linearScale(pm25, 250.5, 500.4, 301, 500);
    } else {
      return 500;
    }
  }

  /**
   * Calculate Air Quality Index (AQI) for PM10
   *
   * @param {number} pm10 - PM10 concentration in µg/m³
   * @returns {number} - AQI value
   */
  calculatePM10AQI(pm10) {
    // Based on EPA AQI calculation for PM10
    if (pm10 <= 54) {
      return this._linearScale(pm10, 0, 54, 0, 50);
    } else if (pm10 <= 154) {
      return this._linearScale(pm10, 55, 154, 51, 100);
    } else if (pm10 <= 254) {
      return this._linearScale(pm10, 155, 254, 101, 150);
    } else if (pm10 <= 354) {
      return this._linearScale(pm10, 255, 354, 151, 200);
    } else if (pm10 <= 424) {
      return this._linearScale(pm10, 355, 424, 201, 300);
    } else if (pm10 <= 604) {
      return this._linearScale(pm10, 425, 604, 301, 500);
    } else {
      return 500;
    }
  }

  /**
   * Calculate Air Quality Index (AQI) for Ozone
   *
   * @param {number} o3 - Ozone concentration in ppb
   * @returns {number} - AQI value
   */
  calculateO3AQI(o3) {
    // Based on EPA AQI calculation for Ozone (8-hour)
    if (o3 <= 54) {
      return this._linearScale(o3, 0, 54, 0, 50);
    } else if (o3 <= 70) {
      return this._linearScale(o3, 55, 70, 51, 100);
    } else if (o3 <= 85) {
      return this._linearScale(o3, 71, 85, 101, 150);
    } else if (o3 <= 105) {
      return this._linearScale(o3, 86, 105, 151, 200);
    } else if (o3 <= 200) {
      return this._linearScale(o3, 106, 200, 201, 300);
    } else {
      return this._linearScale(o3, 201, 604, 301, 500);
    }
  }

  /**
   * Read data through the required physical-device adapter
   *
   * @returns {Promise<Object>} - Device data
   * @private
   */
  async _readDeviceData() {
    const data = await this.adapter.readAirQualityData();
    if (!data || typeof data !== 'object' || Array.isArray(data)) {
      throw new Error('Physical device adapter returned invalid air-quality data');
    }

    const pollutantRanges = {
      pm25: 500.4,
      pm10: 604,
      o3: 604,
    };
    let pollutantCount = 0;
    for (const [pollutant, maximum] of Object.entries(pollutantRanges)) {
      if (data[pollutant] === undefined) continue;
      pollutantCount += 1;
      if (
        typeof data[pollutant] !== 'number' ||
        !Number.isFinite(data[pollutant]) ||
        data[pollutant] < 0 ||
        data[pollutant] > maximum
      ) {
        throw new Error(
          `Physical device adapter returned invalid ${pollutant} concentration`
        );
      }
    }
    if (pollutantCount === 0) {
      throw new Error(
        'Physical device adapter returned no supported pollutant concentrations'
      );
    }

    if (
      data.timestamp === undefined ||
      ((typeof data.timestamp !== 'string' ||
        data.timestamp.trim() === '' ||
        Number.isNaN(Date.parse(data.timestamp))) &&
        (typeof data.timestamp !== 'number' ||
          !Number.isFinite(data.timestamp) ||
          data.timestamp <= 0))
    ) {
      throw new Error('Physical device adapter returned an invalid timestamp');
    }

    if (
      typeof data.rawFrame === 'string' &&
      data.rawFrame.length > 8192
    ) {
      throw new Error('Physical device adapter raw frame exceeds 8192 characters');
    }
    let encoded;
    try {
      encoded = JSON.stringify(data);
    } catch {
      throw new Error('Physical device adapter returned non-serializable data');
    }
    if (!encoded || new TextEncoder().encode(encoded).length > 65_536) {
      throw new Error('Physical device adapter payload exceeds 65536 bytes');
    }
    return data;
  }

  /**
   * Check air quality alerts based on current readings
   *
   * @param {Object} data - Current air quality data
   * @private
   */
  _checkAirQualityAlerts(data) {
    if (!data) return;

    // Calculate AQI for available pollutants
    let highestAQI = 0;
    let criticalPollutant = null;
    let aqi = 0;

    if (data.pm25) {
      aqi = this.calculatePM25AQI(data.pm25);
      if (aqi > highestAQI) {
        highestAQI = aqi;
        criticalPollutant = 'pm25';
      }
    }

    if (data.pm10) {
      aqi = this.calculatePM10AQI(data.pm10);
      if (aqi > highestAQI) {
        highestAQI = aqi;
        criticalPollutant = 'pm10';
      }
    }

    if (data.o3) {
      aqi = this.calculateO3AQI(data.o3);
      if (aqi > highestAQI) {
        highestAQI = aqi;
        criticalPollutant = 'o3';
      }
    }

    // Determine AQI level
    let level = 'good';
    if (highestAQI > this.aqiThresholds.hazardous) {
      level = 'hazardous';
    } else if (highestAQI > this.aqiThresholds.veryUnhealthy) {
      level = 'veryUnhealthy';
    } else if (highestAQI > this.aqiThresholds.unhealthy) {
      level = 'unhealthy';
    } else if (highestAQI > this.aqiThresholds.unhealthyForSensitive) {
      level = 'unhealthyForSensitive';
    } else if (highestAQI > this.aqiThresholds.moderate) {
      level = 'moderate';
    }

    // Check if we should trigger an alert
    if (level !== 'good' && level !== 'moderate') {
      // Create alert data
      const alert = {
        pollutant: criticalPollutant,
        value: data[criticalPollutant],
        aqi: highestAQI,
        level,
        timestamp: data.timestamp,
      };

      // Check if this alert is already active
      const alertKey = `${criticalPollutant}-${level}`;
      if (!this.activeAlerts.includes(alertKey)) {
        // Add to active alerts
        this.activeAlerts.push(alertKey);

        // Submit alert
        this.submitAlert(alert).catch(error => {
          console.error('Failed to submit alert:', error);
        });
      }
    } else {
      // Clear any active alerts
      this.activeAlerts = [];
    }
  }

  /**
   * Linear scale calculation for AQI
   *
   * @param {number} value - The value to transform
   * @param {number} fromLow - Input range low value
   * @param {number} fromHigh - Input range high value
   * @param {number} toLow - Output range low value
   * @param {number} toHigh - Output range high value
   * @returns {number} - Scaled value
   * @private
   */
  _linearScale(value, fromLow, fromHigh, toLow, toHigh) {
    return Math.round(
      ((value - fromLow) * (toHigh - toLow)) / (fromHigh - fromLow) + toLow
    );
  }

  /**
   * Perform sensor calibration
   *
   * @returns {Promise<Object>} - Calibration result
   * @private
   */
  async _performCalibration() {
    if (typeof this.adapter.calibrate !== 'function') {
      throw new Error('Physical device adapter does not implement calibrate()');
    }
    return this.adapter.calibrate();
  }

  /**
   * Add item to offline queue
   *
   * @param {Object} item - Queue item
   * @private
   */
  _addToOfflineQueue(item) {
    if (this.offlineQueue.length >= this.maxQueueSize) {
      // Queue is full, emit event
      this._emitEvent('queue-full', {
        queueSize: this.offlineQueue.length,
        maxSize: this.maxQueueSize,
      });

      // Remove oldest item
      this.offlineQueue.shift();
    }

    this.offlineQueue.push(item);
    console.log(
      `Added item to offline queue. Queue size: ${this.offlineQueue.length}`
    );
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

export { AirQualityMonitorClient };
export default AirQualityMonitorClient;
