<template>
  <div class="weather-station-map">
    <div class="map-header">
      <h3 class="map-title">{{ title }}</h3>
      <div class="map-controls">
        <div v-if="showFilter" class="station-filter">
          <input
            type="text"
            v-model="filterText"
            placeholder="Filter stations..."
            @input="handleFilterChange"
          />
        </div>

        <div v-if="showTypeFilter" class="station-type-filter">
          <select v-model="selectedType" @change="handleTypeFilterChange">
            <option value="all">All Types</option>
            <option
              v-for="type in stationTypes"
              :key="type.value"
              :value="type.value"
            >
              {{ type.label }}
            </option>
          </select>
        </div>

        <div v-if="showRefresh" class="refresh-button" @click="refreshData">
          <slot name="refresh-icon"> ↻ </slot>
        </div>
      </div>
    </div>

    <div v-if="error" class="error-message">
      {{ error }}
    </div>

    <div v-else-if="loading && !stations.length" class="loading-indicator">
      <slot name="loading-indicator"> Loading weather station data... </slot>
    </div>

    <div v-else-if="!stations.length" class="empty-stations">
      <slot name="empty-state">
        <p>No weather stations found</p>
        <p v-if="filterApplied">Try adjusting your filter criteria</p>
      </slot>
    </div>

    <div v-else>
      <div
        class="map-container"
        ref="mapContainer"
        :style="{ height: mapHeight }"
      >
        <!-- Map will be rendered here -->
        <div v-if="!mapRendered" class="map-placeholder">
          <div class="map-placeholder-text">
            <p>
              Interactive map would be rendered here with
              {{ filteredStations.length }} stations.
            </p>
            <p>
              In a real implementation, this would use a mapping library like
              Leaflet or Mapbox.
            </p>
          </div>
        </div>
      </div>

      <div v-if="showStationList" class="station-list">
        <div class="station-count">
          Showing {{ filteredStations.length }} of
          {{ stations.length }} stations
        </div>

        <div class="station-items">
          <div
            v-for="station in filteredStations"
            :key="station.id"
            class="station-item"
            :class="{ active: selectedStationId === station.id }"
            @click="handleStationSelect(station)"
          >
            <div
              class="station-icon"
              :style="{ backgroundColor: getStationColor(station) }"
            >
              {{ station.type.charAt(0).toUpperCase() }}
            </div>

            <div class="station-details">
              <div class="station-name">{{ station.name }}</div>
              <div class="station-location">{{ formatLocation(station) }}</div>
              <div class="station-status">
                <span :class="'status-' + station.status.toLowerCase()">
                  {{ station.status }}
                </span>
                <span v-if="station.lastUpdate" class="station-last-update">
                  Last update: {{ formatTimestamp(station.lastUpdate) }}
                </span>
              </div>
            </div>
          </div>
        </div>
      </div>

      <div v-if="selectedStation" class="station-details-panel">
        <div class="panel-header">
          <h4>{{ selectedStation.name }}</h4>
          <button class="close-button" @click="closeStationDetails">×</button>
        </div>

        <div class="panel-content">
          <div class="detail-item">
            <div class="detail-label">ID</div>
            <div class="detail-value">{{ selectedStation.id }}</div>
          </div>

          <div class="detail-item">
            <div class="detail-label">Type</div>
            <div class="detail-value">
              {{ formatStationType(selectedStation.type) }}
            </div>
          </div>

          <div class="detail-item">
            <div class="detail-label">Location</div>
            <div class="detail-value">
              {{ formatLocation(selectedStation) }}
            </div>
          </div>

          <div class="detail-item">
            <div class="detail-label">Status</div>
            <div class="detail-value">
              <span :class="'status-' + selectedStation.status.toLowerCase()">
                {{ selectedStation.status }}
              </span>
            </div>
          </div>

          <div class="detail-item">
            <div class="detail-label">Last Update</div>
            <div class="detail-value">
              {{ formatTimestamp(selectedStation.lastUpdate) }}
            </div>
          </div>

          <div class="detail-item">
            <div class="detail-label">Elevation</div>
            <div class="detail-value">{{ selectedStation.elevation }} m</div>
          </div>

          <div v-if="selectedStation.metrics" class="station-metrics">
            <h5>Latest Readings</h5>
            <div
              v-for="(value, key) in selectedStation.metrics"
              :key="key"
              class="metric-item"
            >
              <div class="metric-label">{{ formatMetricName(key) }}</div>
              <div class="metric-value">
                {{ formatMetricValue(key, value) }}
              </div>
            </div>
          </div>

          <div class="action-buttons">
            <button
              v-if="onViewData"
              class="view-data-button"
              @click="handleViewData(selectedStation)"
            >
              View Data
            </button>

            <button
              v-if="onViewDetails"
              class="details-button"
              @click="handleViewDetails(selectedStation)"
            >
              Full Details
            </button>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<script>
import { ref, computed, onMounted, watch, nextTick } from 'vue';
import { useWrfcoinStore } from '../../store/WrfcoinStore';

export default {
  name: 'WeatherStationMap',
  props: {
    title: {
      type: String,
      default: 'Weather Stations',
    },
    stationTypes: {
      type: Array,
      default: () => [
        { value: 'official', label: 'Official Stations' },
        { value: 'community', label: 'Community Stations' },
        { value: 'personal', label: 'Personal Stations' },
        { value: 'satellite', label: 'Satellite Data' },
        { value: 'marine', label: 'Marine Stations' },
      ],
    },
    initialStationId: {
      type: String,
      default: null,
    },
    showFilter: {
      type: Boolean,
      default: true,
    },
    showTypeFilter: {
      type: Boolean,
      default: true,
    },
    showRefresh: {
      type: Boolean,
      default: true,
    },
    showStationList: {
      type: Boolean,
      default: true,
    },
    mapHeight: {
      type: String,
      default: '400px',
    },
    refreshInterval: {
      type: Number,
      default: 300000, // 5 minutes
    },
    region: {
      type: String,
      default: null,
    },
    onViewData: {
      type: Function,
      default: null,
    },
    onViewDetails: {
      type: Function,
      default: null,
    },
    onStationSelect: {
      type: Function,
      default: null,
    },
  },
  setup(props, { emit }) {
    const wrfcoinStore = useWrfcoinStore();
    const mapContainer = ref(null);
    const stations = ref([]);
    const loading = ref(false);
    const error = ref(null);
    const filterText = ref('');
    const selectedType = ref('all');
    const selectedStationId = ref(props.initialStationId);
    const selectedStation = ref(null);
    const mapRendered = ref(false);
    const intervalId = ref(null);

    // Computed properties
    const filteredStations = computed(() => {
      return stations.value.filter(station => {
        const typeMatch =
          selectedType.value === 'all' || station.type === selectedType.value;

        if (!filterText.value) {
          return typeMatch;
        }

        const searchTerm = filterText.value.toLowerCase();
        const nameMatch = station.name.toLowerCase().includes(searchTerm);
        const idMatch = station.id.toLowerCase().includes(searchTerm);
        const locationMatch =
          station.location &&
          (station.location.latitude.toString().includes(searchTerm) ||
            station.location.longitude.toString().includes(searchTerm));

        return typeMatch && (nameMatch || idMatch || locationMatch);
      });
    });

    const filterApplied = computed(() => {
      return filterText.value !== '' || selectedType.value !== 'all';
    });

    // Methods
    const fetchData = async () => {
      try {
        loading.value = true;
        error.value = null;

        if (!wrfcoinStore.sdk) {
          throw new Error('SDK is not initialized');
        }

        // Parameters for data fetch
        const params = {};

        // Add region if provided
        if (props.region) {
          params.region = props.region;
        }

        // Fetch weather stations using the SDK
        const stationData = await wrfcoinStore.sdk.getWeatherStations(params);

        if (stationData && Array.isArray(stationData)) {
          stations.value = stationData;
        } else {
          // If we don't get data from the SDK, use mock data for demonstration
          stations.value = generateMockStations();
        }

        await nextTick();
        renderMap();

        // Set selected station if initialStationId is provided
        if (props.initialStationId) {
          const station = stations.value.find(
            s => s.id === props.initialStationId
          );
          if (station) {
            selectedStation.value = station;
          }
        }

        emit('data-loaded', stations.value);
      } catch (err) {
        console.error('Error fetching weather stations:', err);
        error.value = err.message || 'Failed to fetch weather station data';
      } finally {
        loading.value = false;
      }
    };

    const generateMockStations = () => {
      // Generate mock weather stations for demonstration purposes
      const types = [
        'official',
        'community',
        'personal',
        'satellite',
        'marine',
      ];
      const statuses = ['Online', 'Offline', 'Maintenance', 'Limited'];
      const mockStations = [];

      // Generate stations based on region or default to global distribution
      const regions = {
        'North America': {
          lat: { min: 25, max: 49 },
          lng: { min: -125, max: -70 },
        },
        Europe: {
          lat: { min: 36, max: 60 },
          lng: { min: -10, max: 30 },
        },
        Asia: {
          lat: { min: 10, max: 50 },
          lng: { min: 70, max: 140 },
        },
        Australia: {
          lat: { min: -39, max: -10 },
          lng: { min: 113, max: 153 },
        },
        Africa: {
          lat: { min: -30, max: 35 },
          lng: { min: -17, max: 51 },
        },
        'South America': {
          lat: { min: -50, max: 12 },
          lng: { min: -81, max: -35 },
        },
      };

      // Choose region range
      let latRange = { min: -60, max: 75 }; // Global default
      let lngRange = { min: -180, max: 180 };

      if (props.region && regions[props.region]) {
        latRange = regions[props.region].lat;
        lngRange = regions[props.region].lng;
      }

      // Generate random stations
      const stationCount = Math.floor(Math.random() * 20) + 10; // 10-30 stations

      for (let i = 0; i < stationCount; i++) {
        const type = types[Math.floor(Math.random() * types.length)];
        const status = statuses[Math.floor(Math.random() * statuses.length)];
        const latitude =
          latRange.min + Math.random() * (latRange.max - latRange.min);
        const longitude =
          lngRange.min + Math.random() * (lngRange.max - lngRange.min);

        // Randomize update time within the last week
        const lastUpdate = new Date();
        lastUpdate.setHours(
          lastUpdate.getHours() - Math.floor(Math.random() * 168)
        ); // Up to one week ago

        const station = {
          id: `WS-${Math.random().toString(36).substring(2, 10)}`,
          name: `${type.charAt(0).toUpperCase() + type.slice(1)} Station ${i + 1}`,
          type: type,
          status: status,
          location: {
            latitude: parseFloat(latitude.toFixed(4)),
            longitude: parseFloat(longitude.toFixed(4)),
          },
          elevation: Math.floor(Math.random() * 2000),
          lastUpdate: lastUpdate.toISOString(),
        };

        // Add metrics for online stations
        if (status === 'Online' || status === 'Limited') {
          station.metrics = {
            temperature: 15 + (Math.random() * 20 - 10), // 5-25°C
            humidity: Math.floor(Math.random() * 100), // 0-100%
            pressure: 980 + Math.random() * 60, // 980-1040 hPa
            windSpeed: Math.random() * 25, // 0-25 m/s
            rainfall: Math.random() * 5, // 0-5 mm
          };
        }

        mockStations.push(station);
      }

      return mockStations;
    };

    const renderMap = () => {
      if (!mapContainer.value || !stations.value.length) {
        return;
      }

      // This is a placeholder for map rendering
      // In a real implementation, you would use a mapping library
      // like Leaflet, Mapbox, or Google Maps to render the actual map

      // Clear previous map content
      mapContainer.value.innerHTML = '';

      // Create a very simple HTML-based map for demonstration
      const mapEl = document.createElement('div');
      mapEl.style.width = '100%';
      mapEl.style.height = '100%';
      mapEl.style.position = 'relative';
      mapEl.style.backgroundColor = '#f0f5f9';
      mapEl.style.borderRadius = '8px';
      mapEl.style.overflow = 'hidden';

      // Add simple station markers
      filteredStations.value.forEach(station => {
        // Normalize coordinates to the container size
        // This is a very simple linear mapping and not geographically accurate
        const x = ((station.location.longitude + 180) / 360) * 100;
        const y = ((90 - station.location.latitude) / 180) * 100;

        const marker = document.createElement('div');
        marker.style.position = 'absolute';
        marker.style.left = `${x}%`;
        marker.style.top = `${y}%`;
        marker.style.width = '12px';
        marker.style.height = '12px';
        marker.style.borderRadius = '50%';
        marker.style.backgroundColor = getStationColor(station);
        marker.style.border = '2px solid white';
        marker.style.boxShadow = '0 0 4px rgba(0,0,0,0.3)';
        marker.style.transform = 'translate(-50%, -50%)';
        marker.style.cursor = 'pointer';
        marker.title = station.name;

        // Add event listener for marker click
        marker.addEventListener('click', () => {
          handleStationSelect(station);
        });

        // Highlight selected station
        if (selectedStationId.value === station.id) {
          marker.style.width = '16px';
          marker.style.height = '16px';
          marker.style.zIndex = '100';
          marker.style.boxShadow = '0 0 6px rgba(0,0,0,0.5)';
        }

        mapEl.appendChild(marker);
      });

      mapContainer.value.appendChild(mapEl);
      mapRendered.value = true;

      emit('map-rendered');
    };

    const getStationColor = station => {
      // Assign different colors to different station types
      const typeColors = {
        official: '#2ecc71', // green
        community: '#3498db', // blue
        personal: '#9b59b6', // purple
        satellite: '#e74c3c', // red
        marine: '#1abc9c', // teal
      };

      return typeColors[station.type] || '#34495e'; // default color
    };

    const formatLocation = station => {
      if (!station || !station.location) {
        return 'Unknown location';
      }

      const { latitude, longitude } = station.location;

      const latDir = latitude >= 0 ? 'N' : 'S';
      const lngDir = longitude >= 0 ? 'E' : 'W';

      return `${Math.abs(latitude).toFixed(2)}° ${latDir}, ${Math.abs(longitude).toFixed(2)}° ${lngDir}`;
    };

    const formatTimestamp = timestamp => {
      try {
        if (!timestamp) return 'Never';

        const date = new Date(timestamp);
        const now = new Date();
        const diffMs = now - date;

        // Convert to minutes, hours, days
        const diffMins = Math.floor(diffMs / (1000 * 60));
        const diffHours = Math.floor(diffMs / (1000 * 60 * 60));
        const diffDays = Math.floor(diffMs / (1000 * 60 * 60 * 24));

        if (diffMins < 60) {
          return diffMins === 0 ? 'Just now' : `${diffMins}m ago`;
        } else if (diffHours < 24) {
          return `${diffHours}h ago`;
        } else if (diffDays < 7) {
          return `${diffDays}d ago`;
        } else {
          return date.toLocaleDateString();
        }
      } catch (e) {
        return 'Unknown';
      }
    };

    const formatStationType = type => {
      if (!type) return 'Unknown';

      // Find the label from stationTypes prop
      const typeObj = props.stationTypes.find(t => t.value === type);
      if (typeObj) {
        return typeObj.label;
      }

      // Fallback to capitalized type
      return type.charAt(0).toUpperCase() + type.slice(1);
    };

    const formatMetricName = metricKey => {
      const metricNames = {
        temperature: 'Temperature',
        humidity: 'Humidity',
        pressure: 'Pressure',
        windSpeed: 'Wind Speed',
        windDirection: 'Wind Direction',
        rainfall: 'Rainfall',
        solarRadiation: 'Solar Radiation',
        uvIndex: 'UV Index',
        visibility: 'Visibility',
        snowDepth: 'Snow Depth',
      };

      return (
        metricNames[metricKey] ||
        metricKey.charAt(0).toUpperCase() + metricKey.slice(1)
      );
    };

    const formatMetricValue = (metricKey, value) => {
      if (value === null || value === undefined) {
        return '—';
      }

      // Format based on metric type
      switch (metricKey) {
        case 'temperature':
          return `${value.toFixed(1)} °C`;
        case 'humidity':
          return `${value.toFixed(0)} %`;
        case 'pressure':
          return `${value.toFixed(1)} hPa`;
        case 'windSpeed':
          return `${value.toFixed(1)} m/s`;
        case 'windDirection':
          return `${value.toFixed(0)}°`;
        case 'rainfall':
          return `${value.toFixed(1)} mm`;
        case 'solarRadiation':
          return `${value.toFixed(0)} W/m²`;
        case 'uvIndex':
          return value.toFixed(1);
        case 'visibility':
          return `${value.toFixed(1)} km`;
        case 'snowDepth':
          return `${value.toFixed(0)} cm`;
        default:
          return value.toString();
      }
    };

    const handleFilterChange = () => {
      renderMap();
    };

    const handleTypeFilterChange = () => {
      renderMap();
    };

    const handleStationSelect = station => {
      selectedStationId.value = station.id;
      selectedStation.value = station;
      renderMap();

      if (props.onStationSelect) {
        props.onStationSelect(station);
      }

      emit('station-selected', station);
    };

    const closeStationDetails = () => {
      selectedStationId.value = null;
      selectedStation.value = null;
      renderMap();

      emit('station-deselected');
    };

    const handleViewData = station => {
      if (props.onViewData) {
        props.onViewData(station);
      }

      emit('view-data', station);
    };

    const handleViewDetails = station => {
      if (props.onViewDetails) {
        props.onViewDetails(station);
      }

      emit('view-details', station);
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

    // Watch for prop changes
    watch(
      () => props.region,
      () => {
        fetchData();
      }
    );

    // Lifecycle hooks
    onMounted(() => {
      fetchData();
      setupAutoRefresh();
    });

    // Cleanup on unmount (Vue 3 only)
    // In Vue 2, you'd need to use beforeDestroy instead
    // beforeDestroy(() => {
    //   if (intervalId.value) {
    //     clearInterval(intervalId.value);
    //   }
    // });

    return {
      mapContainer,
      stations,
      filteredStations,
      loading,
      error,
      filterText,
      selectedType,
      selectedStationId,
      selectedStation,
      mapRendered,
      filterApplied,
      handleFilterChange,
      handleTypeFilterChange,
      handleStationSelect,
      closeStationDetails,
      handleViewData,
      handleViewDetails,
      refreshData,
      getStationColor,
      formatLocation,
      formatTimestamp,
      formatStationType,
      formatMetricName,
      formatMetricValue,
    };
  },
};
</script>

<style scoped>
.weather-station-map {
  font-family:
    -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Helvetica, Arial,
    sans-serif;
  background-color: #ffffff;
  border-radius: 8px;
  box-shadow: 0 2px 4px rgba(0, 0, 0, 0.1);
  padding: 1rem;
}

.map-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 1rem;
  flex-wrap: wrap;
  gap: 1rem;
}

.map-title {
  font-size: 1.2rem;
  margin: 0;
  color: #333;
}

.map-controls {
  display: flex;
  align-items: center;
  gap: 0.75rem;
  flex-wrap: wrap;
}

.station-filter input {
  padding: 0.25rem 0.5rem;
  border-radius: 4px;
  border: 1px solid #ddd;
  font-size: 0.9rem;
  width: 200px;
}

.station-type-filter select {
  padding: 0.25rem 0.5rem;
  border-radius: 4px;
  border: 1px solid #ddd;
  background-color: #f8f8f8;
  font-size: 0.9rem;
}

.refresh-button {
  cursor: pointer;
  color: #3498db;
  font-size: 1rem;
}

.map-container {
  width: 100%;
  border-radius: 8px;
  overflow: hidden;
  border: 1px solid #eee;
  margin-bottom: 1rem;
}

.map-placeholder {
  width: 100%;
  height: 100%;
  background-color: #f8f8f8;
  display: flex;
  align-items: center;
  justify-content: center;
}

.map-placeholder-text {
  text-align: center;
  color: #666;
}

.station-list {
  margin-top: 1rem;
}

.station-count {
  font-size: 0.85rem;
  color: #777;
  margin-bottom: 0.5rem;
}

.station-items {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(300px, 1fr));
  gap: 1rem;
  max-height: 300px;
  overflow-y: auto;
}

.station-item {
  display: flex;
  align-items: center;
  gap: 0.75rem;
  padding: 0.75rem;
  border-radius: 6px;
  border: 1px solid #eee;
  cursor: pointer;
  transition:
    background-color 0.2s,
    border-color 0.2s;
}

.station-item:hover {
  background-color: #f8f8f8;
  border-color: #ddd;
}

.station-item.active {
  background-color: #f0f7ff;
  border-color: #3498db;
}

.station-icon {
  width: 30px;
  height: 30px;
  border-radius: 50%;
  display: flex;
  align-items: center;
  justify-content: center;
  color: white;
  font-weight: bold;
  flex-shrink: 0;
}

.station-details {
  flex-grow: 1;
  overflow: hidden;
}

.station-name {
  font-weight: 500;
  margin-bottom: 0.25rem;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.station-location {
  font-size: 0.85rem;
  color: #666;
  margin-bottom: 0.25rem;
}

.station-status {
  font-size: 0.8rem;
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.status-online {
  color: #2ecc71;
}

.status-offline {
  color: #e74c3c;
}

.status-maintenance {
  color: #f39c12;
}

.status-limited {
  color: #3498db;
}

.station-last-update {
  color: #777;
}

.station-details-panel {
  margin-top: 1rem;
  border: 1px solid #ddd;
  border-radius: 8px;
  overflow: hidden;
}

.panel-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 0.75rem 1rem;
  background-color: #f8f8f8;
  border-bottom: 1px solid #ddd;
}

.panel-header h4 {
  margin: 0;
  font-size: 1rem;
}

.close-button {
  background: none;
  border: none;
  font-size: 1.2rem;
  cursor: pointer;
  color: #666;
}

.panel-content {
  padding: 1rem;
}

.detail-item {
  display: flex;
  margin-bottom: 0.5rem;
}

.detail-label {
  font-weight: 500;
  width: 100px;
  color: #666;
}

.detail-value {
  flex-grow: 1;
}

.station-metrics {
  margin-top: 1rem;
  border-top: 1px solid #eee;
  padding-top: 0.75rem;
}

.station-metrics h5 {
  margin: 0 0 0.5rem 0;
  font-size: 0.95rem;
  color: #333;
}

.metric-item {
  display: flex;
  margin-bottom: 0.25rem;
}

.metric-label {
  width: 120px;
  color: #666;
}

.metric-value {
  font-weight: 500;
}

.action-buttons {
  display: flex;
  gap: 0.5rem;
  margin-top: 1rem;
  justify-content: flex-end;
}

.action-buttons button {
  padding: 0.4rem 0.75rem;
  border-radius: 4px;
  border: 1px solid #ddd;
  background-color: #f8f8f8;
  cursor: pointer;
  font-size: 0.85rem;
  transition: background-color 0.2s;
}

.action-buttons button:hover {
  background-color: #eee;
}

.view-data-button {
  color: #3498db;
  border-color: #3498db !important;
  background-color: #f0f7ff !important;
}

.loading-indicator,
.error-message,
.empty-stations {
  height: 300px;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  color: #666;
  text-align: center;
}

.error-message {
  color: #e74c3c;
}
</style>
