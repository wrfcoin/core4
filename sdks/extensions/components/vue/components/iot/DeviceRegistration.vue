<template>
  <div class="device-registration">
    <div class="form-header">
      <h3 class="form-title">{{ title }}</h3>
    </div>

    <div v-if="error" class="error-message">
      {{ error }}
    </div>

    <div v-if="success" class="success-message">
      <p>{{ success }}</p>
      <div class="device-details" v-if="registeredDevice">
        <div class="detail-item">
          <div class="detail-label">Device ID:</div>
          <div class="detail-value">{{ registeredDevice.id }}</div>
        </div>
        <div class="detail-item">
          <div class="detail-label">Name:</div>
          <div class="detail-value">{{ registeredDevice.name }}</div>
        </div>
        <div class="detail-item">
          <div class="detail-label">Type:</div>
          <div class="detail-value">
            {{ formatDeviceType(registeredDevice.deviceType) }}
          </div>
        </div>
        <div class="detail-item">
          <div class="detail-label">Status:</div>
          <div
            class="detail-value status-badge"
            :class="'status-' + registeredDevice.status.toLowerCase()"
          >
            {{ registeredDevice.status }}
          </div>
        </div>
      </div>
    </div>

    <form
      v-if="!success"
      class="registration-form"
      @submit.prevent="handleSubmit"
    >
      <div class="form-group">
        <label for="device-name"
          >Device Name <span class="required">*</span></label
        >
        <input
          id="device-name"
          type="text"
          v-model="form.name"
          placeholder="Enter device name"
          :disabled="loading"
          required
        />
      </div>

      <div class="form-group">
        <label for="device-type"
          >Device Type <span class="required">*</span></label
        >
        <select
          id="device-type"
          v-model="form.deviceType"
          :disabled="loading"
          required
        >
          <option value="">Select device type...</option>
          <option
            v-for="type in deviceTypes"
            :key="type.value"
            :value="type.value"
          >
            {{ type.label }}
          </option>
        </select>
      </div>

      <div class="form-group">
        <label for="device-model">Device Model</label>
        <input
          id="device-model"
          type="text"
          v-model="form.model"
          placeholder="Enter device model"
          :disabled="loading"
        />
      </div>

      <div class="form-group">
        <label for="device-firmware">Firmware Version</label>
        <input
          id="device-firmware"
          type="text"
          v-model="form.firmwareVersion"
          placeholder="Enter firmware version"
          :disabled="loading"
        />
      </div>

      <div class="form-group">
        <label for="device-location-lat">Location (Latitude)</label>
        <input
          id="device-location-lat"
          type="number"
          step="0.000001"
          v-model="form.latitude"
          placeholder="Enter latitude (e.g., 40.7128)"
          :disabled="loading"
        />
      </div>

      <div class="form-group">
        <label for="device-location-lng">Location (Longitude)</label>
        <input
          id="device-location-lng"
          type="number"
          step="0.000001"
          v-model="form.longitude"
          placeholder="Enter longitude (e.g., -74.0060)"
          :disabled="loading"
        />
      </div>

      <div class="form-group">
        <label for="device-elevation">Elevation (meters)</label>
        <input
          id="device-elevation"
          type="number"
          v-model="form.elevation"
          placeholder="Enter elevation in meters"
          :disabled="loading"
        />
      </div>

      <div
        v-if="form.deviceType === 'WEATHER_STATION'"
        class="weather-station-details"
      >
        <h4 class="section-title">Weather Station Details</h4>

        <div class="form-group">
          <label>Measured Parameters</label>
          <div class="checkbox-group">
            <label
              class="checkbox-label"
              v-for="param in weatherParameters"
              :key="param.value"
            >
              <input
                type="checkbox"
                v-model="form.weatherParameters"
                :value="param.value"
                :disabled="loading"
              />
              {{ param.label }}
            </label>
          </div>
        </div>

        <div class="form-group">
          <label for="station-update-interval">Update Interval (minutes)</label>
          <input
            id="station-update-interval"
            type="number"
            min="1"
            v-model="form.updateInterval"
            placeholder="Enter update interval"
            :disabled="loading"
          />
        </div>
      </div>

      <div
        v-if="form.deviceType === 'AIR_QUALITY_MONITOR'"
        class="air-quality-details"
      >
        <h4 class="section-title">Air Quality Monitor Details</h4>

        <div class="form-group">
          <label>Measured Pollutants</label>
          <div class="checkbox-group">
            <label
              class="checkbox-label"
              v-for="pollutant in airQualityParameters"
              :key="pollutant.value"
            >
              <input
                type="checkbox"
                v-model="form.airQualityParameters"
                :value="pollutant.value"
                :disabled="loading"
              />
              {{ pollutant.label }}
            </label>
          </div>
        </div>
      </div>

      <div class="form-group">
        <label for="device-notes">Additional Notes</label>
        <textarea
          id="device-notes"
          v-model="form.notes"
          placeholder="Enter any additional notes about this device"
          :disabled="loading"
          rows="3"
        ></textarea>
      </div>

      <div class="form-actions">
        <button
          type="submit"
          class="submit-button"
          :disabled="loading || !isFormValid"
        >
          <span v-if="loading">Registering...</span>
          <span v-else>Register Device</span>
        </button>
      </div>
    </form>
  </div>
</template>

<script>
import { ref, computed, reactive } from 'vue';
import { useWrfcoinStore } from '../../store/WrfcoinStore';

export default {
  name: 'DeviceRegistration',
  props: {
    title: {
      type: String,
      default: 'Register IoT Device',
    },
    deviceTypes: {
      type: Array,
      default: () => [
        { value: 'WEATHER_STATION', label: 'Weather Station' },
        { value: 'AIR_QUALITY_MONITOR', label: 'Air Quality Monitor' },
        { value: 'SOIL_MOISTURE_SENSOR', label: 'Soil Moisture Sensor' },
        { value: 'WATER_LEVEL_SENSOR', label: 'Water Level Sensor' },
        { value: 'GENERIC_SENSOR', label: 'Generic Sensor' },
      ],
    },
    weatherParameters: {
      type: Array,
      default: () => [
        { value: 'temperature', label: 'Temperature' },
        { value: 'humidity', label: 'Humidity' },
        { value: 'pressure', label: 'Pressure' },
        { value: 'windSpeed', label: 'Wind Speed' },
        { value: 'windDirection', label: 'Wind Direction' },
        { value: 'rainfall', label: 'Rainfall' },
        { value: 'solarRadiation', label: 'Solar Radiation' },
        { value: 'uvIndex', label: 'UV Index' },
      ],
    },
    airQualityParameters: {
      type: Array,
      default: () => [
        { value: 'pm25', label: 'PM2.5' },
        { value: 'pm10', label: 'PM10' },
        { value: 'o3', label: 'Ozone (O₃)' },
        { value: 'no2', label: 'Nitrogen Dioxide (NO₂)' },
        { value: 'so2', label: 'Sulfur Dioxide (SO₂)' },
        { value: 'co', label: 'Carbon Monoxide (CO)' },
        { value: 'voc', label: 'Volatile Organic Compounds' },
      ],
    },
    onRegistered: {
      type: Function,
      default: null,
    },
  },
  setup(props, { emit }) {
    const wrfcoinStore = useWrfcoinStore();
    const loading = ref(false);
    const error = ref(null);
    const success = ref(null);
    const registeredDevice = ref(null);

    // Form state
    const form = reactive({
      name: '',
      deviceType: '',
      model: '',
      firmwareVersion: '',
      latitude: null,
      longitude: null,
      elevation: null,
      weatherParameters: [],
      airQualityParameters: [],
      updateInterval: 15, // Default update interval of 15 minutes
      notes: '',
    });

    // Validation
    const isFormValid = computed(() => {
      return !!form.name && !!form.deviceType;
    });

    // Format device type for display
    const formatDeviceType = type => {
      if (!type) return '';

      const deviceType = props.deviceTypes.find(t => t.value === type);
      return deviceType ? deviceType.label : type;
    };

    // Handle form submission
    const handleSubmit = async () => {
      try {
        loading.value = true;
        error.value = null;
        success.value = null;

        if (!wrfcoinStore.sdk) {
          throw new Error('SDK is not initialized');
        }

        // Prepare device data for registration
        const deviceData = {
          name: form.name,
          deviceType: form.deviceType,
          model: form.model || undefined,
          firmwareVersion: form.firmwareVersion || undefined,
          notes: form.notes || undefined,
        };

        // Add location data if provided
        if (form.latitude && form.longitude) {
          deviceData.location = {
            latitude: parseFloat(form.latitude),
            longitude: parseFloat(form.longitude),
          };

          if (form.elevation) {
            deviceData.location.elevation = parseInt(form.elevation, 10);
          }
        }

        // Add specific device type parameters
        if (
          form.deviceType === 'WEATHER_STATION' &&
          form.weatherParameters.length > 0
        ) {
          deviceData.capabilities = {
            parameters: form.weatherParameters,
            updateInterval: parseInt(form.updateInterval, 10) || 15,
          };
        } else if (
          form.deviceType === 'AIR_QUALITY_MONITOR' &&
          form.airQualityParameters.length > 0
        ) {
          deviceData.capabilities = {
            pollutants: form.airQualityParameters,
          };
        }

        // Register the device using the SDK
        const result = await wrfcoinStore.sdk.registerIoTDevice(deviceData);

        if (result && result.id) {
          registeredDevice.value = result;
          success.value = `Device registered successfully with ID: ${result.id}`;

          // Call the onRegistered prop function if provided
          if (props.onRegistered) {
            props.onRegistered(result);
          }

          emit('device-registered', result);
        } else {
          throw new Error('Failed to register device: No device ID returned');
        }
      } catch (err) {
        console.error('Error registering device:', err);
        error.value = err.message || 'Failed to register device';
        emit('registration-error', err);
      } finally {
        loading.value = false;
      }
    };

    return {
      form,
      loading,
      error,
      success,
      registeredDevice,
      isFormValid,
      handleSubmit,
      formatDeviceType,
    };
  },
};
</script>

<style scoped>
.device-registration {
  font-family:
    -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Helvetica, Arial,
    sans-serif;
  background-color: #ffffff;
  border-radius: 8px;
  box-shadow: 0 2px 4px rgba(0, 0, 0, 0.1);
  padding: 1.5rem;
}

.form-header {
  margin-bottom: 1.5rem;
}

.form-title {
  font-size: 1.2rem;
  margin: 0;
  color: #333;
}

.registration-form {
  display: flex;
  flex-direction: column;
  gap: 1rem;
}

.form-group {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
}

.form-group label {
  font-weight: 500;
  color: #333;
  font-size: 0.9rem;
}

.required {
  color: #e74c3c;
}

input,
select,
textarea {
  padding: 0.5rem;
  border: 1px solid #ddd;
  border-radius: 4px;
  font-size: 1rem;
  width: 100%;
}

input:focus,
select:focus,
textarea:focus {
  outline: none;
  border-color: #3498db;
  box-shadow: 0 0 0 2px rgba(52, 152, 219, 0.2);
}

.section-title {
  font-size: 1rem;
  margin: 1rem 0 0.5rem;
  color: #333;
  border-bottom: 1px solid #eee;
  padding-bottom: 0.5rem;
}

.checkbox-group {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem 1rem;
  margin-top: 0.25rem;
}

.checkbox-label {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  font-weight: normal;
  cursor: pointer;
}

.checkbox-label input {
  width: auto;
}

.form-actions {
  margin-top: 1rem;
  display: flex;
  justify-content: flex-end;
}

.submit-button {
  background-color: #3498db;
  color: white;
  border: none;
  border-radius: 4px;
  padding: 0.5rem 1rem;
  font-size: 1rem;
  cursor: pointer;
  transition: background-color 0.2s;
}

.submit-button:hover {
  background-color: #2980b9;
}

.submit-button:disabled {
  background-color: #95a5a6;
  cursor: not-allowed;
}

.error-message {
  background-color: #feeaea;
  color: #e74c3c;
  padding: 0.75rem;
  border-radius: 4px;
  margin-bottom: 1rem;
  font-size: 0.9rem;
}

.success-message {
  background-color: #e8f7f0;
  color: #2ecc71;
  padding: 0.75rem;
  border-radius: 4px;
  margin-bottom: 1rem;
  font-size: 0.9rem;
}

.device-details {
  margin-top: 1rem;
  background-color: #f8f8f8;
  border-radius: 4px;
  padding: 1rem;
}

.detail-item {
  display: flex;
  margin-bottom: 0.5rem;
}

.detail-label {
  font-weight: 500;
  width: 120px;
  color: #666;
}

.detail-value {
  flex-grow: 1;
}

.status-badge {
  display: inline-block;
  padding: 0.25rem 0.5rem;
  border-radius: 10px;
  font-size: 0.8rem;
}

.status-active {
  background-color: #e8f7f0;
  color: #2ecc71;
}

.status-inactive {
  background-color: #f8f8f8;
  color: #95a5a6;
}

.status-maintenance {
  background-color: #fef8e8;
  color: #f39c12;
}

.status-error {
  background-color: #feeaea;
  color: #e74c3c;
}

@media (max-width: 768px) {
  .device-registration {
    padding: 1rem;
  }

  .detail-item {
    flex-direction: column;
  }

  .detail-label {
    width: auto;
    margin-bottom: 0.25rem;
  }
}
</style>
