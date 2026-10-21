/**
 * WeatherStationClient
 *
 * This module provides a client for connecting weather stations to the Wrfcoin network.
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

class WeatherStationClient {
  /**
   * Create a new WeatherStationClient
   *
   * @param {Object} options - Configuration options
   * @param {Object} options.sdk - Wrfcoin SDK instance
   * @param {Object} options.adapter - Physical weather-station adapter
   * @param {Function} options.adapter.readWeatherData - Read a physical observation
   * @param {Object} [options.deviceInfo] - Device information
   * @param {string} [options.deviceInfo.name] - Device name
   * @param {string} [options.deviceInfo.model] - Device model
   * @param {string} [options.deviceInfo.firmwareVersion] - Device firmware version
   * @param {Object} [options.deviceInfo.location] - Device location
   * @param {number} [options.deviceInfo.location.latitude] - Location latitude
   * @param {number} [options.deviceInfo.location.longitude] - Location longitude
   * @param {number} [options.deviceInfo.location.elevation] - Location elevation in meters
   * @param {string[]} [options.deviceInfo.parameters] - Parameters this device can measure
   * @param {number} [options.updateInterval=15] - Data update interval in minutes
   * @param {string} [options.network='testnet'] - Network to use
   * @param {number} [options.maxQueueSize=100] - Maximum offline submissions retained
   * @param {boolean} [options.autoRetry=true] - Retry queued submissions automatically
   * @param {number} [options.retryDelay=60000] - Retry delay in milliseconds
   */
  constructor(options) {
    if (!options || !options.sdk) {
      throw new Error('Wrfcoin SDK instance is required');
    }
    if (!options.adapter || typeof options.adapter.readWeatherData !== 'function') {
      throw new Error(
        'A physical device adapter with readWeatherData() is required'
      );
    }

    this.sdk = options.sdk;
    this.adapter = options.adapter;
    this.deviceInfo = options.deviceInfo || {};
    this.deviceId = null;
    this.registered = false;
    this.updateInterval = options.updateInterval || 15;
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
      'legacy-format-detected': [],
    };
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
        deviceType: 'WEATHER_STATION',
        model: this.deviceInfo.model,
        firmwareVersion: this.deviceInfo.firmwareVersion,
        location: this.deviceInfo.location,
        capabilities: {
          parameters: this.deviceInfo.parameters || [
            'temperature',
            'humidity',
            'pressure',
          ],
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
        5 * 60 * 1000
      ); // Every 5 minutes
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

      const {
        canonicalPayload,
        legacyKeysDetected,
        observedAtIsoTimestamp,
      } = this._canonicalizeWeatherData(data);

      if (legacyKeysDetected.length > 0) {
        const telemetry = {
          deviceId: this.deviceId,
          stationId: canonicalPayload.station_id || null,
          schemaVersion: canonicalPayload.schema_version,
          legacyKeys: legacyKeysDetected,
        };
        this._emitEvent('legacy-format-detected', telemetry);
        console.warn(
          `Legacy weather payload keys detected (${legacyKeysDetected.join(', ')}). Emitting canonical schema v1 payload.`
        );
      }

      // Prepare submission payload
      const payload = {
        deviceId: this.deviceId,
        data: canonicalPayload,
        timestamp: observedAtIsoTimestamp,
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
      const { isOnline, batteryLevel, sensorStatus } = deviceStatus;
      if (
        typeof isOnline !== 'boolean' ||
        typeof batteryLevel !== 'number' ||
        !Number.isFinite(batteryLevel) ||
        batteryLevel < 0 ||
        batteryLevel > 100 ||
        !sensorStatus ||
        typeof sensorStatus.allOperational !== 'boolean' ||
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
      } else if (batteryLevel < 10) {
        status = 'error';
        message = `Low battery: ${batteryLevel}%`;
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
   * Update device firmware
   *
   * @param {string} firmwareUrl - URL to the new firmware
   * @returns {Promise<Object>} - Update result
   */
  async updateFirmware(firmwareUrl) {
    try {
      if (!this.registered || !this.deviceId) {
        throw new Error('Device not registered');
      }

      if (!firmwareUrl) {
        throw new Error('Firmware URL is required');
      }
      if (typeof this.sdk.updateDeviceInfo !== 'function') {
        throw new Error('Wrfcoin SDK must implement updateDeviceInfo()');
      }

      // Set device to maintenance mode
      await this.updateStatus('maintenance', 'Updating firmware');

      // Implement actual firmware update logic here
      const updateResult = await this._performFirmwareUpdate(firmwareUrl);

      if (
        !updateResult ||
        typeof updateResult !== 'object' ||
        Array.isArray(updateResult) ||
        typeof updateResult.success !== 'boolean'
      ) {
        throw new Error('Physical device adapter returned an invalid firmware result');
      }
      if (
        updateResult.success &&
        (typeof updateResult.version !== 'string' ||
          updateResult.version.trim() === '' ||
          updateResult.version.length > 128)
      ) {
        throw new Error(
          'Physical device adapter returned success without a valid firmware version'
        );
      }
      const firmwareError = updateResult.success
        ? ''
        : sanitizeStatusText(updateResult.error, 1024, 'firmware error');

      // Update firmware version in device info
      if (updateResult.success) {
        this.deviceInfo.firmwareVersion = updateResult.version;

        // Update device info on blockchain
        await this.sdk.updateDeviceInfo({
          deviceId: this.deviceId,
          firmwareVersion: updateResult.version,
          network: this.network,
        });

        // Set device back to active
        await this.updateStatus('active', 'Firmware updated successfully');
      } else {
        // Set device to error state
        await this.updateStatus(
          'error',
          `Firmware update failed: ${firmwareError}`
        );
      }

      return updateResult;
    } catch (error) {
      // Set device to error state
      await this.updateStatus(
        'error',
        `Firmware update failed: ${error.message}`
      ).catch(() => {});

      this._emitEvent('error', {
        operation: 'update-firmware',
        message: error.message,
        error,
      });
      throw error;
    }
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
   * Convert legacy/canonical weather readings into canonical schema v1 payload.
   *
   * @param {Object} reading - input weather reading
   * @returns {{canonicalPayload: Object, legacyKeysDetected: string[], observedAtIsoTimestamp: string}}
   * @private
   */
  _canonicalizeWeatherData(reading) {
    if (!reading || typeof reading !== 'object' || Array.isArray(reading)) {
      throw new Error('Weather reading must be an object');
    }

    const source =
      reading.observation && typeof reading.observation === 'object'
        ? reading.observation
        : reading;
    const legacyKeysDetected = [];
    const observation = {};

    const temperature = this._pickNumericField(source, [
      'temperature_c',
      'temperature',
      'temp_c',
      'temperature_celsius',
    ]);
    if (temperature !== null) {
      observation.temperature_c = temperature.value;
      if (temperature.key !== 'temperature_c') legacyKeysDetected.push(temperature.key);
    }

    const humidity = this._pickNumericField(source, ['humidity', 'relative_humidity']);
    if (humidity !== null) {
      observation.humidity = humidity.value;
      if (humidity.key !== 'humidity') legacyKeysDetected.push(humidity.key);
    }

    const pressure = this._pickNumericField(source, ['pressure_hpa', 'pressure']);
    if (pressure !== null) {
      observation.pressure_hpa = pressure.value;
      if (pressure.key !== 'pressure_hpa') legacyKeysDetected.push(pressure.key);
    }

    const windSpeed = this._pickNumericField(source, [
      'wind_speed_ms',
      'wind_speed',
      'windSpeed',
    ]);
    if (windSpeed !== null) {
      observation.wind_speed_ms = windSpeed.value;
      if (windSpeed.key !== 'wind_speed_ms') legacyKeysDetected.push(windSpeed.key);
    }

    const precipitation = this._pickNumericField(source, [
      'precipitation',
      'precipitation_mm',
      'rainfall',
      'precip_mm',
    ]);
    if (precipitation !== null) {
      observation.precipitation = precipitation.value;
      if (precipitation.key !== 'precipitation')
        legacyKeysDetected.push(precipitation.key);
    }

    const dewpoint = this._pickNumericField(source, [
      'dewpoint_c',
      'dewpoint',
      'dew_point',
    ]);
    if (dewpoint !== null) {
      observation.dewpoint_c = dewpoint.value;
      if (dewpoint.key !== 'dewpoint_c') legacyKeysDetected.push(dewpoint.key);
    }

    if (Object.keys(observation).length === 0) {
      throw new Error(
        'Weather reading must include at least one recognized weather field'
      );
    }

    const stationId =
      this._pickTextField(reading, ['station_id', 'stationId']) ||
      this.deviceInfo.stationId ||
      this.deviceId;
    // The observation time is data: a reading without one is refused, never stamped with "now".
    const observedAtUnix =
      this._pickIntegerField(reading, ['observed_at_unix', 'observedAtUnix']) ||
      this._toUnixSeconds(reading.timestamp);
    if (!observedAtUnix || observedAtUnix <= 0) {
      const missing = new Error(
        'Weather reading must carry an observation timestamp (observed_at_unix or timestamp); none is invented'
      );
      missing.nonRetryable = true;
      throw missing;
    }

    const canonicalPayload = {
      schema_version: 'wrf.weather.observation.v1',
      station_id: stationId,
      observed_at_unix: observedAtUnix,
      observation,
    };

    return {
      canonicalPayload,
      legacyKeysDetected: [...new Set(legacyKeysDetected)],
      observedAtIsoTimestamp: new Date(observedAtUnix * 1000).toISOString(),
    };
  }

  /**
   * Pick the first numeric field from candidate keys.
   *
   * @param {Object} source
   * @param {string[]} keys
   * @returns {{key: string, value: number} | null}
   * @private
   */
  _pickNumericField(source, keys) {
    for (const key of keys) {
      if (!Object.prototype.hasOwnProperty.call(source, key)) continue;
      const value = source[key];
      const parsed = this._toNumber(value);
      if (parsed !== null) {
        return { key, value: parsed };
      }
    }
    return null;
  }

  /**
   * Pick the first string field from candidate keys.
   *
   * @param {Object} source
   * @param {string[]} keys
   * @returns {string | null}
   * @private
   */
  _pickTextField(source, keys) {
    for (const key of keys) {
      if (!Object.prototype.hasOwnProperty.call(source, key)) continue;
      const value = source[key];
      if (typeof value === 'string' && value.trim() !== '') {
        return value.trim();
      }
    }
    return null;
  }

  /**
   * Pick first integer-valued field from candidate keys.
   *
   * @param {Object} source
   * @param {string[]} keys
   * @returns {number | null}
   * @private
   */
  _pickIntegerField(source, keys) {
    for (const key of keys) {
      if (!Object.prototype.hasOwnProperty.call(source, key)) continue;
      const value = source[key];
      const parsed = this._toInteger(value);
      if (parsed !== null) return parsed;
    }
    return null;
  }

  /**
   * Convert value to finite number.
   *
   * @param {unknown} value
   * @returns {number | null}
   * @private
   */
  _toNumber(value) {
    if (typeof value === 'number' && Number.isFinite(value)) return value;
    if (typeof value === 'string' && value.trim() !== '') {
      const parsed = Number.parseFloat(value);
      if (Number.isFinite(parsed)) return parsed;
    }
    return null;
  }

  /**
   * Convert value to integer.
   *
   * @param {unknown} value
   * @returns {number | null}
   * @private
   */
  _toInteger(value) {
    if (typeof value === 'number' && Number.isFinite(value)) {
      return Math.trunc(value);
    }
    if (typeof value === 'string' && value.trim() !== '') {
      const parsed = Number.parseInt(value, 10);
      if (Number.isFinite(parsed)) return parsed;
    }
    return null;
  }

  /**
   * Convert timestamp-like values into unix seconds.
   *
   * @param {unknown} value
   * @returns {number | null}
   * @private
   */
  _toUnixSeconds(value) {
    if (value instanceof Date && !Number.isNaN(value.getTime())) {
      return Math.floor(value.getTime() / 1000);
    }
    if (typeof value === 'number' && Number.isFinite(value)) {
      // Treat 13-digit values as milliseconds.
      return value > 9999999999 ? Math.floor(value / 1000) : Math.floor(value);
    }
    if (typeof value === 'string' && value.trim() !== '') {
      const numeric = Number(value);
      if (Number.isFinite(numeric)) {
        return numeric > 9999999999
          ? Math.floor(numeric / 1000)
          : Math.floor(numeric);
      }
      const parsed = Date.parse(value);
      if (!Number.isNaN(parsed)) return Math.floor(parsed / 1000);
    }
    return null;
  }

  /**
   * Read data through the required physical-device adapter
   *
   * @returns {Promise<Object>} - Device data
   * @private
   */
  async _readDeviceData() {
    const data = await this.adapter.readWeatherData();
    if (!data || typeof data !== 'object' || Array.isArray(data)) {
      throw new Error('Physical device adapter returned invalid weather data');
    }
    this._validatePhysicalWeatherData(data);
    return data;
  }

  /**
   * Validate an untrusted physical observation before canonicalization can
   * default or omit malformed fields.
   *
   * @param {Object} data
   * @private
   */
  _validatePhysicalWeatherData(data) {
    const source =
      data.observation && typeof data.observation === 'object' &&
      !Array.isArray(data.observation)
        ? data.observation
        : data;
    const fields = [
      [['temperature_c', 'temperature', 'temp_c', 'temperature_celsius'], -89.2, 56.7, 'temperature'],
      [['humidity', 'relative_humidity'], 0, 100, 'humidity'],
      [['pressure_hpa', 'pressure'], 870, 1084, 'pressure'],
      [['wind_speed_ms', 'wind_speed', 'windSpeed'], 0, 113.2, 'wind speed'],
      [['precipitation', 'precipitation_mm', 'rainfall', 'precip_mm'], 0, Number.MAX_VALUE, 'precipitation'],
      [['dewpoint_c', 'dewpoint', 'dew_point'], -89.2, 56.7, 'dewpoint'],
    ];
    let observedFields = 0;
    for (const [keys, minimum, maximum, label] of fields) {
      for (const key of keys) {
        if (!Object.prototype.hasOwnProperty.call(source, key)) continue;
        observedFields += 1;
        const value = source[key];
        if (typeof value !== 'number' || !Number.isFinite(value) || value < minimum || value > maximum) {
          throw new Error(`Physical device adapter returned invalid ${label}`);
        }
        break;
      }
    }
    if (observedFields === 0) {
      throw new Error('Physical device adapter returned no supported weather fields');
    }

    const timestamp =
      data.observed_at_unix ?? data.observedAtUnix ?? data.timestamp;
    const observedAtUnix = this._toUnixSeconds(timestamp);
    if (timestamp === undefined || observedAtUnix === null || observedAtUnix <= 0) {
      throw new Error('Physical device adapter returned an invalid timestamp');
    }

    const stationId = data.station_id ?? data.stationId;
    if (
      stationId !== undefined &&
      (typeof stationId !== 'string' || stationId.trim() === '' || stationId.length > 256)
    ) {
      throw new Error('Physical device adapter returned an invalid station identifier');
    }
    if (typeof data.rawFrame === 'string' && data.rawFrame.length > 8192) {
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
  }

  /**
   * Perform firmware update
   *
   * @param {string} firmwareUrl - URL to the new firmware
   * @returns {Promise<Object>} - Update result
   * @private
   */
  async _performFirmwareUpdate(firmwareUrl) {
    if (typeof this.adapter.updateFirmware !== 'function') {
      throw new Error(
        'Physical device adapter does not implement updateFirmware()'
      );
    }
    return this.adapter.updateFirmware(firmwareUrl);
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

export { WeatherStationClient };
export default WeatherStationClient;
