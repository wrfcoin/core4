<template>
  <div class="weather-data-visualization">
    <div class="chart-header">
      <h3 class="chart-title">{{ title }}</h3>
      <div class="chart-controls">
        <div
          v-if="showDeviceSelector && devices.length > 1"
          class="device-selector"
        >
          <label for="device-select">Device:</label>
          <select
            id="device-select"
            v-model="selectedDeviceId"
            @change="handleDeviceChange"
          >
            <option
              v-for="device in devices"
              :key="device.id"
              :value="device.id"
            >
              {{ device.name }}
            </option>
          </select>
        </div>

        <div
          v-if="showParameterSelector && availableParameters.length > 0"
          class="parameter-selector"
        >
          <label for="parameter-select">Parameter:</label>
          <select
            id="parameter-select"
            v-model="selectedParameter"
            @change="handleParameterChange"
          >
            <option
              v-for="param in availableParameters"
              :key="param.value"
              :value="param.value"
            >
              {{ param.label }}
            </option>
          </select>
        </div>

        <div v-if="showTimeRangeSelector" class="time-range-selector">
          <button
            v-for="range in timeRanges"
            :key="range.value"
            :class="{ active: selectedTimeRange === range.value }"
            @click="handleTimeRangeChange(range.value)"
          >
            {{ range.label }}
          </button>
        </div>

        <div v-if="showRefresh" class="refresh-button" @click="refreshData">
          <slot name="refresh-icon"> ↻ </slot>
        </div>
      </div>
    </div>

    <div v-if="error" class="error-message">
      {{ error }}
    </div>

    <div v-else-if="loading && !data.length" class="loading-indicator">
      <slot name="loading-indicator"> Loading weather data... </slot>
    </div>

    <div v-else-if="!data.length" class="empty-data">
      <slot name="empty-state">
        <p>No data available for the selected parameter and time range</p>
      </slot>
    </div>

    <div v-else>
      <div
        class="graph-container"
        ref="graphContainer"
        :style="{ height: graphHeight }"
      >
        <!-- Graph will be rendered here -->
        <div v-if="!graphRendered" class="graph-placeholder">
          <div class="graph-placeholder-text">
            <p>Interactive graph would be rendered here.</p>
            <p>
              {{ data.length }} data points for
              {{ getParameterLabel(selectedParameter) }}.
            </p>
          </div>
        </div>
      </div>

      <div v-if="showStatistics" class="data-statistics">
        <div class="statistic-item">
          <div class="statistic-label">Current</div>
          <div class="statistic-value">{{ formatValue(currentValue) }}</div>
        </div>

        <div class="statistic-item">
          <div class="statistic-label">Average</div>
          <div class="statistic-value">{{ formatValue(averageValue) }}</div>
        </div>

        <div class="statistic-item">
          <div class="statistic-label">Min</div>
          <div class="statistic-value">{{ formatValue(minValue) }}</div>
        </div>

        <div class="statistic-item">
          <div class="statistic-label">Max</div>
          <div class="statistic-value">{{ formatValue(maxValue) }}</div>
        </div>
      </div>

      <div class="selected-device-info" v-if="selectedDevice">
        <div class="device-info-header">
          <div class="device-name">{{ selectedDevice.name }}</div>
          <div
            :class="
              'device-status status-' + selectedDevice.status.toLowerCase()
            "
          >
            {{ selectedDevice.status }}
          </div>
        </div>

        <div v-if="selectedDevice.location" class="device-location">
          <i class="location-icon">📍</i>
          {{ formatLocation(selectedDevice.location) }}
        </div>

        <div v-if="lastUpdated" class="last-updated">
          Last updated: {{ formatTimestamp(lastUpdated) }}
        </div>
      </div>
    </div>
  </div>
</template>

<script>
import { ref, computed, onMounted, watch, nextTick } from 'vue';
import { useWrfcoinStore } from '../../store/WrfcoinStore';

export default {
  name: 'WeatherDataVisualization',
  props: {
    title: {
      type: String,
      default: 'Weather Data',
    },
    deviceId: {
      type: String,
      default: null, // If provided, will show data for this specific device
    },
    deviceIds: {
      type: Array,
      default: () => [], // If provided, will show data for these devices only
    },
    parameters: {
      type: Array,
      default: () => [
        { value: 'temperature', label: 'Temperature (°C)' },
        { value: 'humidity', label: 'Humidity (%)' },
        { value: 'pressure', label: 'Pressure (hPa)' },
        { value: 'windSpeed', label: 'Wind Speed (m/s)' },
        { value: 'windDirection', label: 'Wind Direction (°)' },
        { value: 'rainfall', label: 'Rainfall (mm)' },
        { value: 'solarRadiation', label: 'Solar Radiation (W/m²)' },
        { value: 'uvIndex', label: 'UV Index' },
      ],
    },
    defaultParameter: {
      type: String,
      default: 'temperature',
    },
    showDeviceSelector: {
      type: Boolean,
      default: true,
    },
    showParameterSelector: {
      type: Boolean,
      default: true,
    },
    showTimeRangeSelector: {
      type: Boolean,
      default: true,
    },
    timeRanges: {
      type: Array,
      default: () => [
        { value: '1h', label: '1H' },
        { value: '24h', label: '24H' },
        { value: '7d', label: '7D' },
        { value: '30d', label: '30D' },
      ],
    },
    defaultTimeRange: {
      type: String,
      default: '24h',
    },
    showRefresh: {
      type: Boolean,
      default: true,
    },
    showStatistics: {
      type: Boolean,
      default: true,
    },
    graphHeight: {
      type: String,
      default: '300px',
    },
    refreshInterval: {
      type: Number,
      default: 60000, // 1 minute
    },
    network: {
      type: String,
      default: null, // If null, uses the current connected network
    },
  },
  setup(props, { emit }) {
    const wrfcoinStore = useWrfcoinStore();
    const graphContainer = ref(null);
    const data = ref([]);
    const devices = ref([]);
    const loading = ref(false);
    const error = ref(null);
    const selectedDeviceId = ref(props.deviceId);
    const selectedParameter = ref(props.defaultParameter);
    const selectedTimeRange = ref(props.defaultTimeRange);
    const graphRendered = ref(false);
    const lastUpdated = ref(null);
    const intervalId = ref(null);
    const wsHandlerId = ref(null);

    // Computed properties
    const selectedDevice = computed(() => {
      if (!selectedDeviceId.value) return null;
      return devices.value.find(d => d.id === selectedDeviceId.value);
    });

    const availableParameters = computed(() => {
      if (
        !selectedDevice.value ||
        !selectedDevice.value.capabilities ||
        !Array.isArray(selectedDevice.value.capabilities.parameters)
      ) {
        return props.parameters;
      }

      // Filter parameters based on device capabilities
      return props.parameters.filter(param =>
        selectedDevice.value.capabilities.parameters.includes(param.value)
      );
    });

    const currentValue = computed(() => {
      if (!data.value.length) {
        return null;
      }
      return data.value[data.value.length - 1].value;
    });

    const averageValue = computed(() => {
      if (!data.value.length) {
        return null;
      }

      const sum = data.value.reduce((acc, item) => {
        return acc + item.value;
      }, 0);

      return sum / data.value.length;
    });

    const minValue = computed(() => {
      if (!data.value.length) {
        return null;
      }

      return Math.min(...data.value.map(item => item.value));
    });

    const maxValue = computed(() => {
      if (!data.value.length) {
        return null;
      }

      return Math.max(...data.value.map(item => item.value));
    });

    // Methods
    const fetchDevices = async () => {
      try {
        if (!wrfcoinStore.sdk) {
          throw new Error('SDK is not initialized');
        }

        const params = {
          deviceType: 'WEATHER_STATION',
        };

        if (props.deviceIds && props.deviceIds.length > 0) {
          params.deviceIds = props.deviceIds;
        }

        if (props.network) {
          params.network = props.network;
        }

        const deviceData = await wrfcoinStore.sdk.getIoTDevices(params);

        if (deviceData && Array.isArray(deviceData)) {
          devices.value = deviceData;
        } else {
          // If we don't get data from the SDK, use mock data for demonstration
          devices.value = generateMockDevices();
        }

        // If no device is selected, select the first one
        if (!selectedDeviceId.value && devices.value.length > 0) {
          selectedDeviceId.value = devices.value[0].id;
        }

        emit('devices-loaded', devices.value);
      } catch (err) {
        console.error('Error fetching devices:', err);
        error.value = err.message || 'Failed to fetch weather stations';
      }
    };

    const fetchData = async () => {
      try {
        loading.value = true;
        error.value = null;

        if (!wrfcoinStore.sdk) {
          throw new Error('SDK is not initialized');
        }

        if (!selectedDeviceId.value) {
          throw new Error('No device selected');
        }

        // Parameters for data fetch
        const params = {
          deviceId: selectedDeviceId.value,
          parameter: selectedParameter.value,
          timeRange: selectedTimeRange.value,
        };

        // Add network if provided
        if (props.network) {
          params.network = props.network;
        }

        // Fetch weather data using the SDK
        const weatherData = await wrfcoinStore.sdk.getDeviceData(params);

        if (weatherData && Array.isArray(weatherData)) {
          data.value = weatherData;
        } else {
          // If we don't get data from the SDK, use mock data for demonstration
          data.value = generateMockData();
        }

        lastUpdated.value = new Date();
        await nextTick();
        renderGraph();

        emit('data-loaded', data.value);
      } catch (err) {
        console.error('Error fetching weather data:', err);
        error.value = err.message || 'Failed to fetch weather data';
      } finally {
        loading.value = false;
      }
    };

    const generateMockDevices = () => {
      // Generate mock weather station devices for demonstration
      const statuses = ['Active', 'Inactive', 'Maintenance', 'Error'];
      const mockDevices = [];

      // Generate random devices
      const deviceCount = 3; // Generate 3 devices

      for (let i = 0; i < deviceCount; i++) {
        const status = statuses[Math.floor(Math.random() * statuses.length)];

        const device = {
          id: `ws-${Math.random().toString(36).substring(2, 10)}`,
          name: `Weather Station ${i + 1}`,
          deviceType: 'WEATHER_STATION',
          model: `Model WS-${Math.floor(Math.random() * 1000)}`,
          status: status,
        };

        // Add location
        device.location = {
          latitude: (Math.random() * 180 - 90).toFixed(6),
          longitude: (Math.random() * 360 - 180).toFixed(6),
          elevation: Math.floor(Math.random() * 1000),
        };

        // Add capabilities
        const parameters = [
          'temperature',
          'humidity',
          'pressure',
          'windSpeed',
          'windDirection',
          'rainfall',
        ];
        device.capabilities = {
          parameters: parameters,
          updateInterval: Math.floor(Math.random() * 15) + 5,
        };

        mockDevices.push(device);
      }

      return mockDevices;
    };

    const generateMockData = () => {
      // Generate mock weather data for demonstration
      const now = new Date();
      const mockData = [];

      // Determine number of data points based on time range
      let dataPoints = 24; // Default for 24h
      let timeStep = 60 * 60 * 1000; // 1 hour in milliseconds

      switch (selectedTimeRange.value) {
        case '1h':
          dataPoints = 60;
          timeStep = 60 * 1000; // 1 minute
          break;
        case '7d':
          dataPoints = 7 * 24;
          timeStep = 60 * 60 * 1000; // 1 hour
          break;
        case '30d':
          dataPoints = 30;
          timeStep = 24 * 60 * 60 * 1000; // 1 day
          break;
        default: // 24h
          dataPoints = 24;
          timeStep = 60 * 60 * 1000; // 1 hour
      }

      // Generate data based on selected parameter
      let baseValue = 0;
      let maxVariation = 0;
      let decimalPlaces = 0;

      switch (selectedParameter.value) {
        case 'temperature':
          baseValue = 20; // 20°C
          maxVariation = 10; // +/- 10°C
          decimalPlaces = 1;
          break;
        case 'humidity':
          baseValue = 65; // 65%
          maxVariation = 25; // +/- 25%
          decimalPlaces = 0;
          break;
        case 'pressure':
          baseValue = 1013; // 1013 hPa
          maxVariation = 15; // +/- 15 hPa
          decimalPlaces = 1;
          break;
        case 'windSpeed':
          baseValue = 5; // 5 m/s
          maxVariation = 7; // +/- 7 m/s
          decimalPlaces = 1;
          break;
        case 'windDirection':
          baseValue = 180; // 180°
          maxVariation = 180; // +/- 180°
          decimalPlaces = 0;
          break;
        case 'rainfall':
          baseValue = 0; // 0 mm
          maxVariation = 5; // +/- 5 mm
          decimalPlaces = 1;
          break;
        case 'solarRadiation':
          baseValue = 500; // 500 W/m²
          maxVariation = 500; // +/- 500 W/m²
          decimalPlaces = 0;
          break;
        case 'uvIndex':
          baseValue = 5; // Index 5
          maxVariation = 5; // +/- 5
          decimalPlaces = 0;
          break;
        default:
          baseValue = 20;
          maxVariation = 10;
          decimalPlaces = 1;
      }

      // Generate smooth random data with some patterns
      for (let i = 0; i < dataPoints; i++) {
        const timestamp = new Date(now.getTime() - (dataPoints - i) * timeStep);

        // Generate a value with some pattern based on time of day
        const hour = timestamp.getHours();
        const dayFactor = Math.sin((hour / 24) * Math.PI * 2); // Day/night cycle

        // Add random variation with time-based pattern
        const randomFactor = Math.random() * 0.5 - 0.25; // -0.25 to 0.25
        const cyclicFactor = Math.sin((i / dataPoints) * Math.PI * 2); // Cyclic pattern

        // Combine factors
        let value =
          baseValue +
          (dayFactor + randomFactor + cyclicFactor * 0.5) * maxVariation;

        // Special handling for some parameters
        if (selectedParameter.value === 'humidity') {
          value = Math.min(Math.max(value, 0), 100); // Humidity between 0-100%
        } else if (selectedParameter.value === 'windDirection') {
          value = (value + 360) % 360; // Wind direction between 0-360°
        } else if (selectedParameter.value === 'rainfall') {
          // Rainfall is more sporadic with many zero values
          if (Math.random() < 0.7) {
            value = 0;
          } else {
            value = Math.max(0, value); // Rainfall cannot be negative
          }
        } else if (selectedParameter.value === 'uvIndex') {
          // UV Index is 0 at night
          if (hour < 6 || hour > 19) {
            value = 0;
          }
          value = Math.max(0, Math.min(11, value)); // UV Index between 0-11
        }

        mockData.push({
          timestamp: timestamp.toISOString(),
          value: parseFloat(value.toFixed(decimalPlaces)),
        });
      }

      return mockData;
    };

    const renderGraph = () => {
      if (!graphContainer.value || !data.value.length) {
        return;
      }

      // This is a placeholder for graph rendering
      // In a real implementation, you would use a charting library
      // like Chart.js, D3.js, or Vue-specific libraries to render the actual graph

      // Clear previous graph content
      graphContainer.value.innerHTML = '';

      // Create a very simple HTML-based graph for demonstration
      const graphEl = document.createElement('div');
      graphEl.style.height = props.graphHeight;
      graphEl.style.display = 'flex';
      graphEl.style.alignItems = 'flex-end';
      graphEl.style.gap = '2px';

      // Get the data values
      const values = data.value.map(item => item.value);
      const minVal = Math.min(...values);
      const maxVal = Math.max(...values);
      const valueRange = maxVal - minVal;

      values.forEach((value, index) => {
        // Calculate percentage height (with a 5% minimum for visibility)
        const percentage =
          valueRange > 0 ? 5 + ((value - minVal) / valueRange) * 95 : 50;

        const bar = document.createElement('div');
        bar.style.height = `${percentage}%`;
        bar.style.width = `${100 / values.length}%`;
        bar.style.backgroundColor = getParameterColor(selectedParameter.value);
        bar.style.borderRadius = '2px 2px 0 0';

        // Add tooltip with value and timestamp
        const timestamp = new Date(
          data.value[index].timestamp
        ).toLocaleTimeString();
        bar.title = `${formatValue(value)} (${timestamp})`;

        graphEl.appendChild(bar);
      });

      graphContainer.value.appendChild(graphEl);
      graphRendered.value = true;

      emit('graph-rendered');
    };

    const getParameterColor = parameter => {
      // Assign different colors to different parameters
      const colors = {
        temperature: '#e74c3c', // red
        humidity: '#3498db', // blue
        pressure: '#9b59b6', // purple
        windSpeed: '#2ecc71', // green
        windDirection: '#f39c12', // orange
        rainfall: '#1abc9c', // teal
        solarRadiation: '#f1c40f', // yellow
        uvIndex: '#e67e22', // orange
      };

      return colors[parameter] || '#34495e'; // default color
    };

    const getParameterLabel = parameter => {
      const paramObj = props.parameters.find(p => p.value === parameter);
      return paramObj ? paramObj.label : parameter;
    };

    const getParameterUnit = parameter => {
      switch (parameter) {
        case 'temperature':
          return '°C';
        case 'humidity':
          return '%';
        case 'pressure':
          return 'hPa';
        case 'windSpeed':
          return 'm/s';
        case 'windDirection':
          return '°';
        case 'rainfall':
          return 'mm';
        case 'solarRadiation':
          return 'W/m²';
        case 'uvIndex':
          return '';
        default:
          return '';
      }
    };

    const formatValue = value => {
      if (value === null || value === undefined) {
        return '—';
      }

      // Format based on selected parameter
      const unit = getParameterUnit(selectedParameter.value);

      switch (selectedParameter.value) {
        case 'temperature':
          return `${value.toFixed(1)}${unit}`;
        case 'humidity':
          return `${Math.round(value)}${unit}`;
        case 'pressure':
          return `${value.toFixed(1)}${unit}`;
        case 'windSpeed':
          return `${value.toFixed(1)}${unit}`;
        case 'windDirection':
          return `${Math.round(value)}${unit}`;
        case 'rainfall':
          return `${value.toFixed(1)}${unit}`;
        case 'solarRadiation':
          return `${Math.round(value)}${unit}`;
        case 'uvIndex':
          return `${Math.round(value)}`;
        default:
          return `${value}${unit}`;
      }
    };

    const formatTimestamp = timestamp => {
      try {
        if (!timestamp) return '';
        const date = new Date(timestamp);
        return date.toLocaleString();
      } catch (e) {
        return timestamp;
      }
    };

    const formatLocation = location => {
      if (!location) return '';

      let locationString = '';

      if (location.latitude && location.longitude) {
        locationString = `${parseFloat(location.latitude).toFixed(4)}, ${parseFloat(location.longitude).toFixed(4)}`;
      }

      if (location.elevation) {
        locationString += ` (${location.elevation}m)`;
      }

      return locationString;
    };

    const handleDeviceChange = () => {
      fetchData();
      emit('device-change', selectedDeviceId.value);
    };

    const handleParameterChange = () => {
      fetchData();
      emit('parameter-change', selectedParameter.value);
    };

    const handleTimeRangeChange = range => {
      selectedTimeRange.value = range;
      fetchData();
      emit('time-range-change', range);
    };

    const refreshData = () => {
      fetchData();
    };

    // Setup auto-refresh
    const setupAutoRefresh = () => {
      clearInterval(intervalId.value);

      if (props.refreshInterval > 0) {
        intervalId.value = setInterval(() => {
          fetchData();
        }, props.refreshInterval);
      }
    };

    // Setup WebSocket listener for live updates
    const setupWebSocketListener = () => {
      if (!wrfcoinStore.sdk || !wrfcoinStore.sdk.subscribeToDeviceData) {
        console.warn('SDK does not support device data subscriptions');
        return;
      }

      if (!selectedDeviceId.value) {
        return;
      }

      removeWebSocketListener(); // Remove existing listener if any

      const params = {
        deviceId: selectedDeviceId.value,
        parameter: selectedParameter.value,
      };

      if (props.network) {
        params.network = props.network;
      }

      wsHandlerId.value = wrfcoinStore.sdk.subscribeToDeviceData(
        newDataPoint => {
          // Add the new data point
          data.value.push(newDataPoint);

          // Remove oldest data point if we exceed the desired time range
          const oldestAllowed = new Date();
          let timeRangeMs = 24 * 60 * 60 * 1000; // Default 24h

          switch (selectedTimeRange.value) {
            case '1h':
              timeRangeMs = 60 * 60 * 1000;
              break;
            case '7d':
              timeRangeMs = 7 * 24 * 60 * 60 * 1000;
              break;
            case '30d':
              timeRangeMs = 30 * 24 * 60 * 60 * 1000;
              break;
          }

          oldestAllowed.setTime(oldestAllowed.getTime() - timeRangeMs);

          // Filter out data points older than the time range
          data.value = data.value.filter(point => {
            const pointDate = new Date(point.timestamp);
            return pointDate >= oldestAllowed;
          });

          // Update the graph
          renderGraph();

          lastUpdated.value = new Date();
          emit('data-updated', newDataPoint);
        },
        params
      );
    };

    // Remove WebSocket listener
    const removeWebSocketListener = () => {
      if (
        wsHandlerId.value &&
        wrfcoinStore.sdk &&
        wrfcoinStore.sdk.unsubscribeFromDeviceData
      ) {
        wrfcoinStore.sdk.unsubscribeFromDeviceData(wsHandlerId.value);
        wsHandlerId.value = null;
      }
    };

    // Watch for prop changes
    watch(
      () => props.deviceId,
      newDeviceId => {
        if (newDeviceId) {
          selectedDeviceId.value = newDeviceId;
          fetchData();
          setupWebSocketListener();
        }
      }
    );

    watch(
      () => selectedDeviceId.value,
      () => {
        setupWebSocketListener();
      }
    );

    watch(
      () => selectedParameter.value,
      () => {
        setupWebSocketListener();
      }
    );

    watch(
      () => props.network,
      () => {
        fetchDevices();
        fetchData();
        setupWebSocketListener();
      }
    );

    // Lifecycle hooks
    onMounted(() => {
      fetchDevices().then(() => {
        fetchData();
      });
      setupAutoRefresh();
      setupWebSocketListener();
    });

    // Cleanup on unmount (Vue 3 only)
    // In Vue 2, you'd need to use beforeDestroy instead
    // beforeDestroy(() => {
    //   if (intervalId.value) {
    //     clearInterval(intervalId.value);
    //   }
    //   removeWebSocketListener();
    // });

    return {
      graphContainer,
      data,
      devices,
      selectedDeviceId,
      selectedDevice,
      selectedParameter,
      selectedTimeRange,
      loading,
      error,
      graphRendered,
      lastUpdated,
      availableParameters,
      currentValue,
      averageValue,
      minValue,
      maxValue,
      handleDeviceChange,
      handleParameterChange,
      handleTimeRangeChange,
      refreshData,
      getParameterLabel,
      formatValue,
      formatTimestamp,
      formatLocation,
    };
  },
};
</script>

<style scoped>
.weather-data-visualization {
  font-family:
    -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Helvetica, Arial,
    sans-serif;
  background-color: #ffffff;
  border-radius: 8px;
  box-shadow: 0 2px 4px rgba(0, 0, 0, 0.1);
  padding: 1rem;
}

.chart-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 1rem;
  flex-wrap: wrap;
  gap: 1rem;
}

.chart-title {
  font-size: 1.2rem;
  margin: 0;
  color: #333;
}

.chart-controls {
  display: flex;
  align-items: center;
  gap: 0.75rem;
  flex-wrap: wrap;
}

.device-selector select,
.parameter-selector select {
  padding: 0.25rem 0.5rem;
  border-radius: 4px;
  border: 1px solid #ddd;
  background-color: #f8f8f8;
  font-size: 0.9rem;
}

.time-range-selector {
  display: flex;
  gap: 0.25rem;
}

.time-range-selector button {
  border: 1px solid #ddd;
  background-color: #f8f8f8;
  color: #666;
  border-radius: 4px;
  padding: 0.25rem 0.5rem;
  font-size: 0.8rem;
  cursor: pointer;
}

.time-range-selector button.active {
  background-color: #3498db;
  border-color: #3498db;
  color: white;
}

.refresh-button {
  cursor: pointer;
  color: #3498db;
  font-size: 1rem;
}

.graph-container {
  width: 100%;
  border-radius: 8px;
  overflow: hidden;
  border: 1px solid #eee;
  margin-bottom: 1rem;
}

.graph-placeholder {
  width: 100%;
  height: 100%;
  background-color: #f8f8f8;
  display: flex;
  align-items: center;
  justify-content: center;
}

.graph-placeholder-text {
  text-align: center;
  color: #666;
}

.data-statistics {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(100px, 1fr));
  gap: 1rem;
  margin-bottom: 1rem;
}

.statistic-item {
  display: flex;
  flex-direction: column;
  align-items: center;
}

.statistic-label {
  font-size: 0.8rem;
  color: #666;
  margin-bottom: 0.25rem;
}

.statistic-value {
  font-size: 1.2rem;
  font-weight: 600;
  color: #333;
}

.selected-device-info {
  border-top: 1px solid #eee;
  padding-top: 1rem;
  margin-top: 1rem;
}

.device-info-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 0.5rem;
}

.device-name {
  font-weight: 500;
  font-size: 1rem;
  color: #333;
}

.device-status {
  font-size: 0.8rem;
  padding: 0.1rem 0.4rem;
  border-radius: 10px;
  white-space: nowrap;
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

.device-location {
  font-size: 0.85rem;
  color: #666;
  margin-bottom: 0.5rem;
  display: flex;
  align-items: center;
  gap: 0.25rem;
}

.location-icon {
  font-style: normal;
  color: #3498db;
}

.last-updated {
  font-size: 0.8rem;
  color: #777;
  margin-top: 0.5rem;
  text-align: right;
}

.loading-indicator,
.error-message,
.empty-data {
  height: 300px;
  display: flex;
  align-items: center;
  justify-content: center;
  color: #666;
}

.error-message {
  color: #e74c3c;
}
</style>
