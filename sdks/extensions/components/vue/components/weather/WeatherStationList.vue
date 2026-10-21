<template>
  <div class="weather-station-list">
    <div class="list-header">
      <h3 class="list-title">{{ title }}</h3>
      <div class="list-controls">
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

        <div v-if="showRegionFilter" class="region-filter">
          <select v-model="selectedRegion" @change="handleRegionFilterChange">
            <option value="all">All Regions</option>
            <option
              v-for="region in regions"
              :key="region.value"
              :value="region.value"
            >
              {{ region.label }}
            </option>
          </select>
        </div>

        <div v-if="showStatusFilter" class="status-filter">
          <select v-model="selectedStatus" @change="handleStatusFilterChange">
            <option value="all">All Statuses</option>
            <option value="online">Online</option>
            <option value="offline">Offline</option>
            <option value="maintenance">Maintenance</option>
            <option value="limited">Limited</option>
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

    <div v-else-if="!filteredStations.length" class="empty-stations">
      <slot name="empty-state">
        <p>No weather stations found</p>
        <p v-if="filterApplied">Try adjusting your filter criteria</p>
      </slot>
    </div>

    <div v-else>
      <div class="station-count">
        Showing {{ filteredStations.length }} of {{ stations.length }} stations
      </div>

      <div class="filter-summary" v-if="filterApplied">
        <div class="filter-tags">
          <div v-if="selectedType !== 'all'" class="filter-tag">
            Type: {{ formatStationType(selectedType) }}
            <span class="remove-filter" @click="selectedType = 'all'">×</span>
          </div>

          <div v-if="selectedRegion !== 'all'" class="filter-tag">
            Region: {{ formatRegion(selectedRegion) }}
            <span class="remove-filter" @click="selectedRegion = 'all'">×</span>
          </div>

          <div v-if="selectedStatus !== 'all'" class="filter-tag">
            Status:
            {{
              selectedStatus.charAt(0).toUpperCase() + selectedStatus.slice(1)
            }}
            <span class="remove-filter" @click="selectedStatus = 'all'">×</span>
          </div>

          <div v-if="filterText" class="filter-tag">
            Search: "{{ filterText }}"
            <span class="remove-filter" @click="filterText = ''">×</span>
          </div>

          <div
            v-if="filterApplied"
            class="clear-all-filters"
            @click="clearAllFilters"
          >
            Clear All Filters
          </div>
        </div>
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
            <div class="station-name-row">
              <div class="station-name">{{ station.name }}</div>
              <div
                :class="'station-status status-' + station.status.toLowerCase()"
              >
                {{ station.status }}
              </div>
            </div>

            <div class="station-location">{{ formatLocation(station) }}</div>

            <div class="station-metrics" v-if="station.metrics">
              <div
                v-for="metric in displayMetrics"
                :key="metric.key"
                class="station-metric"
                v-if="station.metrics[metric.key] !== undefined"
              >
                <span class="metric-label">{{ metric.label }}:</span>
                <span class="metric-value">{{
                  formatMetricValue(metric.key, station.metrics[metric.key])
                }}</span>
              </div>
            </div>

            <div class="station-footer">
              <div class="station-id">ID: {{ formatId(station.id) }}</div>
              <div v-if="station.lastUpdate" class="station-last-update">
                {{ formatTimestamp(station.lastUpdate) }}
              </div>
            </div>
          </div>

          <div class="station-actions">
            <button
              v-if="onViewData"
              class="action-button view-data-button"
              @click.stop="handleViewData(station)"
              title="View Station Data"
            >
              <slot name="view-data-icon"> 📊 </slot>
            </button>

            <button
              v-if="onViewDetails"
              class="action-button details-button"
              @click.stop="handleViewDetails(station)"
              title="View Station Details"
            >
              <slot name="view-details-icon"> ℹ️ </slot>
            </button>
          </div>
        </div>
      </div>

      <div class="pagination" v-if="totalPages > 1">
        <button
          class="pagination-button"
          :disabled="currentPage === 1"
          @click="handlePageChange(currentPage - 1)"
        >
          Previous
        </button>

        <div class="page-numbers">
          <button
            v-for="page in visiblePageNumbers"
            :key="page"
            class="page-number"
            :class="{ active: currentPage === page }"
            @click="handlePageChange(page)"
          >
            {{ page }}
          </button>
        </div>

        <button
          class="pagination-button"
          :disabled="currentPage === totalPages"
          @click="handlePageChange(currentPage + 1)"
        >
          Next
        </button>
      </div>
    </div>
  </div>
</template>

<script>
import { ref, computed, onMounted, watch } from 'vue';
import { useWrfcoinStore } from '../../store/WrfcoinStore';

export default {
  name: 'WeatherStationList',
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
    regions: {
      type: Array,
      default: () => [
        { value: 'north_america', label: 'North America' },
        { value: 'south_america', label: 'South America' },
        { value: 'europe', label: 'Europe' },
        { value: 'africa', label: 'Africa' },
        { value: 'asia', label: 'Asia' },
        { value: 'oceania', label: 'Oceania' },
        { value: 'antarctica', label: 'Antarctica' },
      ],
    },
    displayMetrics: {
      type: Array,
      default: () => [
        { key: 'temperature', label: 'Temp' },
        { key: 'humidity', label: 'Humid' },
        { key: 'windSpeed', label: 'Wind' },
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
    showRegionFilter: {
      type: Boolean,
      default: true,
    },
    showStatusFilter: {
      type: Boolean,
      default: true,
    },
    showRefresh: {
      type: Boolean,
      default: true,
    },
    itemsPerPage: {
      type: Number,
      default: 10,
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
    const stations = ref([]);
    const loading = ref(false);
    const error = ref(null);
    const filterText = ref('');
    const selectedType = ref('all');
    const selectedRegion = ref(props.region || 'all');
    const selectedStatus = ref('all');
    const selectedStationId = ref(props.initialStationId);
    const currentPage = ref(1);
    const intervalId = ref(null);

    // Computed properties
    const filteredStations = computed(() => {
      return stations.value.filter(station => {
        const typeMatch =
          selectedType.value === 'all' || station.type === selectedType.value;
        const regionMatch =
          selectedRegion.value === 'all' ||
          (station.region && station.region === selectedRegion.value);
        const statusMatch =
          selectedStatus.value === 'all' ||
          (station.status &&
            station.status.toLowerCase() ===
              selectedStatus.value.toLowerCase());

        if (!filterText.value) {
          return typeMatch && regionMatch && statusMatch;
        }

        const searchTerm = filterText.value.toLowerCase();
        const nameMatch = station.name.toLowerCase().includes(searchTerm);
        const idMatch = station.id.toLowerCase().includes(searchTerm);
        const locationMatch =
          station.location &&
          (station.location.latitude.toString().includes(searchTerm) ||
            station.location.longitude.toString().includes(searchTerm));

        return (
          typeMatch &&
          regionMatch &&
          statusMatch &&
          (nameMatch || idMatch || locationMatch)
        );
      });
    });

    const filterApplied = computed(() => {
      return (
        filterText.value !== '' ||
        selectedType.value !== 'all' ||
        selectedRegion.value !== 'all' ||
        selectedStatus.value !== 'all'
      );
    });

    const paginatedStations = computed(() => {
      const startIndex = (currentPage.value - 1) * props.itemsPerPage;
      const endIndex = startIndex + props.itemsPerPage;
      return filteredStations.value.slice(startIndex, endIndex);
    });

    const totalPages = computed(() => {
      return Math.ceil(filteredStations.value.length / props.itemsPerPage);
    });

    const visiblePageNumbers = computed(() => {
      if (totalPages.value <= 7) {
        return Array.from({ length: totalPages.value }, (_, i) => i + 1);
      }

      if (currentPage.value <= 4) {
        return [1, 2, 3, 4, 5, '...', totalPages.value];
      }

      if (currentPage.value >= totalPages.value - 3) {
        return [
          1,
          '...',
          totalPages.value - 4,
          totalPages.value - 3,
          totalPages.value - 2,
          totalPages.value - 1,
          totalPages.value,
        ];
      }

      return [
        1,
        '...',
        currentPage.value - 1,
        currentPage.value,
        currentPage.value + 1,
        '...',
        totalPages.value,
      ];
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

        // Set selected station if initialStationId is provided
        if (props.initialStationId) {
          const station = stations.value.find(
            s => s.id === props.initialStationId
          );
          if (station) {
            selectedStationId.value = station.id;
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
      const regionValues = props.regions.map(r => r.value);
      const mockStations = [];

      // Generate random stations
      const stationCount = Math.floor(Math.random() * 50) + 30; // 30-80 stations

      for (let i = 0; i < stationCount; i++) {
        const type = types[Math.floor(Math.random() * types.length)];
        const status = statuses[Math.floor(Math.random() * statuses.length)];
        const region =
          regionValues[Math.floor(Math.random() * regionValues.length)];

        // Generate coordinates based on region
        let latitude, longitude;

        switch (region) {
          case 'north_america':
            latitude = 25 + Math.random() * 25; // 25-50 N
            longitude = -125 + Math.random() * 55; // 125-70 W
            break;
          case 'south_america':
            latitude = -50 + Math.random() * 60; // 50 S to 10 N
            longitude = -81 + Math.random() * 45; // 81-36 W
            break;
          case 'europe':
            latitude = 36 + Math.random() * 25; // 36-61 N
            longitude = -10 + Math.random() * 40; // 10 W to 30 E
            break;
          case 'africa':
            latitude = -35 + Math.random() * 70; // 35 S to 35 N
            longitude = -17 + Math.random() * 70; // 17 W to 53 E
            break;
          case 'asia':
            latitude = 10 + Math.random() * 50; // 10-60 N
            longitude = 30 + Math.random() * 120; // 30-150 E
            break;
          case 'oceania':
            latitude = -45 + Math.random() * 35; // 45-10 S
            longitude = 110 + Math.random() * 70; // 110-180 E
            break;
          case 'antarctica':
            latitude = -75 + Math.random() * 10; // 75-65 S
            longitude = -180 + Math.random() * 360; // All longitudes
            break;
          default:
            latitude = -60 + Math.random() * 120; // 60 S to 60 N
            longitude = -180 + Math.random() * 360; // All longitudes
        }

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
          region: region,
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

    const formatRegion = regionValue => {
      if (!regionValue) return 'Unknown';

      // Find the label from regions prop
      const regionObj = props.regions.find(r => r.value === regionValue);
      if (regionObj) {
        return regionObj.label;
      }

      // Fallback to capitalized region with underscores replaced
      return regionValue
        .split('_')
        .map(word => word.charAt(0).toUpperCase() + word.slice(1))
        .join(' ');
    };

    const formatMetricValue = (metricKey, value) => {
      if (value === null || value === undefined) {
        return '—';
      }

      // Format based on metric type
      switch (metricKey) {
        case 'temperature':
          return `${value.toFixed(1)}°C`;
        case 'humidity':
          return `${value.toFixed(0)}%`;
        case 'pressure':
          return `${value.toFixed(0)} hPa`;
        case 'windSpeed':
          return `${value.toFixed(1)} m/s`;
        case 'rainfall':
          return `${value.toFixed(1)} mm`;
        default:
          return value.toString();
      }
    };

    const formatId = id => {
      if (!id) return '';

      // If ID is long, truncate it
      if (id.length > 12) {
        return `${id.substring(0, 6)}...${id.substring(id.length - 6)}`;
      }

      return id;
    };

    const handleFilterChange = () => {
      currentPage.value = 1; // Reset to first page when filter changes
    };

    const handleTypeFilterChange = () => {
      currentPage.value = 1;
    };

    const handleRegionFilterChange = () => {
      currentPage.value = 1;
    };

    const handleStatusFilterChange = () => {
      currentPage.value = 1;
    };

    const clearAllFilters = () => {
      filterText.value = '';
      selectedType.value = 'all';
      selectedRegion.value = 'all';
      selectedStatus.value = 'all';
      currentPage.value = 1;
    };

    const handleStationSelect = station => {
      selectedStationId.value = station.id;

      if (props.onStationSelect) {
        props.onStationSelect(station);
      }

      emit('station-selected', station);
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

    const handlePageChange = page => {
      if (typeof page === 'number') {
        currentPage.value = page;
        emit('page-change', page);
      }
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
        if (props.region) {
          selectedRegion.value = props.region;
        }
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
      stations,
      filteredStations,
      paginatedStations,
      loading,
      error,
      filterText,
      selectedType,
      selectedRegion,
      selectedStatus,
      selectedStationId,
      currentPage,
      totalPages,
      visiblePageNumbers,
      filterApplied,
      handleFilterChange,
      handleTypeFilterChange,
      handleRegionFilterChange,
      handleStatusFilterChange,
      clearAllFilters,
      handleStationSelect,
      handleViewData,
      handleViewDetails,
      handlePageChange,
      refreshData,
      getStationColor,
      formatLocation,
      formatTimestamp,
      formatStationType,
      formatRegion,
      formatMetricValue,
      formatId,
    };
  },
};
</script>

<style scoped>
.weather-station-list {
  font-family:
    -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Helvetica, Arial,
    sans-serif;
  background-color: #ffffff;
  border-radius: 8px;
  box-shadow: 0 2px 4px rgba(0, 0, 0, 0.1);
  padding: 1rem;
}

.list-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 1rem;
  flex-wrap: wrap;
  gap: 1rem;
}

.list-title {
  font-size: 1.2rem;
  margin: 0;
  color: #333;
}

.list-controls {
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

.station-type-filter select,
.region-filter select,
.status-filter select {
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

.station-count {
  font-size: 0.85rem;
  color: #777;
  margin-bottom: 0.5rem;
}

.filter-summary {
  margin-bottom: 1rem;
}

.filter-tags {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem;
}

.filter-tag {
  display: inline-flex;
  align-items: center;
  background-color: #f0f7ff;
  color: #3498db;
  padding: 0.25rem 0.5rem;
  border-radius: 4px;
  font-size: 0.85rem;
}

.remove-filter {
  margin-left: 0.25rem;
  cursor: pointer;
  font-weight: bold;
}

.clear-all-filters {
  cursor: pointer;
  color: #e74c3c;
  font-size: 0.85rem;
  padding: 0.25rem 0.5rem;
}

.station-items {
  display: flex;
  flex-direction: column;
  gap: 1rem;
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
  width: 40px;
  height: 40px;
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

.station-name-row {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 0.25rem;
}

.station-name {
  font-weight: 500;
  font-size: 1rem;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.station-status {
  font-size: 0.8rem;
  padding: 0.1rem 0.4rem;
  border-radius: 10px;
  white-space: nowrap;
}

.status-online {
  background-color: #e8f7f0;
  color: #2ecc71;
}

.status-offline {
  background-color: #feeaea;
  color: #e74c3c;
}

.status-maintenance {
  background-color: #fef8e8;
  color: #f39c12;
}

.status-limited {
  background-color: #ebf5fb;
  color: #3498db;
}

.station-location {
  color: #666;
  font-size: 0.9rem;
  margin-bottom: 0.25rem;
}

.station-metrics {
  display: flex;
  gap: 1rem;
  margin-bottom: 0.25rem;
}

.station-metric {
  font-size: 0.85rem;
}

.metric-label {
  color: #666;
}

.metric-value {
  font-weight: 500;
  margin-left: 0.25rem;
}

.station-footer {
  display: flex;
  justify-content: space-between;
  font-size: 0.8rem;
  color: #777;
}

.station-actions {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
}

.action-button {
  background: none;
  border: none;
  cursor: pointer;
  font-size: 1rem;
  padding: 0.25rem;
  border-radius: 4px;
  transition: background-color 0.2s;
}

.action-button:hover {
  background-color: rgba(0, 0, 0, 0.05);
}

.view-data-button {
  color: #3498db;
}

.details-button {
  color: #95a5a6;
}

.pagination {
  display: flex;
  justify-content: center;
  align-items: center;
  margin-top: 1.5rem;
  gap: 0.5rem;
}

.pagination-button {
  padding: 0.4rem 0.8rem;
  border: 1px solid #ddd;
  background-color: #f8f8f8;
  border-radius: 4px;
  cursor: pointer;
  font-size: 0.85rem;
}

.pagination-button:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.page-numbers {
  display: flex;
  gap: 0.25rem;
}

.page-number {
  width: 2rem;
  height: 2rem;
  display: flex;
  align-items: center;
  justify-content: center;
  border: 1px solid #ddd;
  background-color: #f8f8f8;
  border-radius: 4px;
  cursor: pointer;
  font-size: 0.85rem;
}

.page-number.active {
  background-color: #3498db;
  color: white;
  border-color: #3498db;
}

.loading-indicator,
.error-message,
.empty-stations {
  padding: 2rem 0;
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
