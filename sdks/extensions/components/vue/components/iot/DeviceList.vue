<template>
  <div class="device-list">
    <div class="list-header">
      <h3 class="list-title">{{ title }}</h3>
      <div class="list-controls">
        <div v-if="showFilter" class="device-filter">
          <input
            type="text"
            v-model="filterText"
            placeholder="Filter devices..."
            @input="handleFilterChange"
          />
        </div>

        <div v-if="showTypeFilter" class="device-type-filter">
          <select v-model="selectedType" @change="handleTypeFilterChange">
            <option value="all">All Types</option>
            <option
              v-for="type in deviceTypes"
              :key="type.value"
              :value="type.value"
            >
              {{ type.label }}
            </option>
          </select>
        </div>

        <div v-if="showStatusFilter" class="status-filter">
          <select v-model="selectedStatus" @change="handleStatusFilterChange">
            <option value="all">All Statuses</option>
            <option value="active">Active</option>
            <option value="inactive">Inactive</option>
            <option value="maintenance">Maintenance</option>
            <option value="error">Error</option>
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

    <div v-else-if="loading && !devices.length" class="loading-indicator">
      <slot name="loading-indicator"> Loading device data... </slot>
    </div>

    <div v-else-if="!filteredDevices.length" class="empty-devices">
      <slot name="empty-state">
        <p>No devices found</p>
        <p v-if="filterApplied">Try adjusting your filter criteria</p>
      </slot>
    </div>

    <div v-else>
      <div class="device-count">
        Showing {{ filteredDevices.length }} of {{ devices.length }} devices
      </div>

      <div class="filter-summary" v-if="filterApplied">
        <div class="filter-tags">
          <div v-if="selectedType !== 'all'" class="filter-tag">
            Type: {{ formatDeviceType(selectedType) }}
            <span class="remove-filter" @click="selectedType = 'all'">×</span>
          </div>

          <div v-if="selectedStatus !== 'all'" class="filter-tag">
            Status: {{ formatStatus(selectedStatus) }}
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

      <div class="device-items">
        <div
          v-for="device in paginatedDevices"
          :key="device.id"
          class="device-item"
          :class="{ active: selectedDeviceId === device.id }"
          @click="handleDeviceSelect(device)"
        >
          <div
            class="device-icon"
            :style="{ backgroundColor: getDeviceColor(device) }"
          >
            {{ device.deviceType.charAt(0).toUpperCase() }}
          </div>

          <div class="device-details">
            <div class="device-id-row">
              <div class="device-name">{{ device.name }}</div>
              <div
                :class="'device-status status-' + device.status.toLowerCase()"
              >
                {{ device.status }}
              </div>
            </div>

            <div class="device-type-id">
              <span class="device-type">{{
                formatDeviceType(device.deviceType)
              }}</span>
              <span class="device-id">ID: {{ formatId(device.id) }}</span>
            </div>

            <div v-if="device.location" class="device-location">
              <i class="location-icon">📍</i>
              {{ formatLocation(device.location) }}
            </div>

            <div class="device-footer">
              <div v-if="device.lastReported" class="device-last-reported">
                Last reported: {{ formatTimestamp(device.lastReported) }}
              </div>
              <div v-if="isWeatherStation(device)" class="device-parameters">
                {{ formatParameters(device.capabilities?.parameters) }}
              </div>
            </div>
          </div>

          <div class="device-actions">
            <button
              v-if="onViewDetails"
              class="action-button details-button"
              @click.stop="handleViewDetails(device)"
              title="View Device Details"
            >
              <slot name="view-details-icon"> ℹ️ </slot>
            </button>

            <button
              v-if="onViewData"
              class="action-button data-button"
              @click.stop="handleViewData(device)"
              title="View Device Data"
            >
              <slot name="view-data-icon"> 📊 </slot>
            </button>

            <button
              v-if="onEditDevice"
              class="action-button edit-button"
              @click.stop="handleEditDevice(device)"
              title="Edit Device"
            >
              <slot name="edit-icon"> ✏️ </slot>
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

      <div v-if="showLiveUpdates" class="live-updates-toggle">
        <label class="toggle-label">
          <input
            type="checkbox"
            v-model="liveUpdates"
            @change="handleLiveUpdatesChange"
          />
          <span class="toggle-text">Live Updates</span>
        </label>
      </div>
    </div>
  </div>
</template>

<script>
import { ref, computed, onMounted, watch } from 'vue';
import { useWrfcoinStore } from '../../store/WrfcoinStore';

export default {
  name: 'DeviceList',
  props: {
    title: {
      type: String,
      default: 'IoT Devices',
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
    initialDeviceId: {
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
    showStatusFilter: {
      type: Boolean,
      default: true,
    },
    showRefresh: {
      type: Boolean,
      default: true,
    },
    showLiveUpdates: {
      type: Boolean,
      default: true,
    },
    itemsPerPage: {
      type: Number,
      default: 10,
    },
    refreshInterval: {
      type: Number,
      default: 30000, // 30 seconds
    },
    ownerId: {
      type: String,
      default: null, // If provided, show only devices for this owner
    },
    network: {
      type: String,
      default: null, // If null, uses the current connected network
    },
    onViewDetails: {
      type: Function,
      default: null,
    },
    onViewData: {
      type: Function,
      default: null,
    },
    onEditDevice: {
      type: Function,
      default: null,
    },
    onDeviceSelect: {
      type: Function,
      default: null,
    },
  },
  setup(props, { emit }) {
    const wrfcoinStore = useWrfcoinStore();
    const devices = ref([]);
    const loading = ref(false);
    const error = ref(null);
    const filterText = ref('');
    const selectedType = ref('all');
    const selectedStatus = ref('all');
    const selectedDeviceId = ref(props.initialDeviceId);
    const currentPage = ref(1);
    const liveUpdates = ref(true);
    const intervalId = ref(null);
    const wsHandlerId = ref(null);

    // Computed properties
    const filteredDevices = computed(() => {
      return devices.value.filter(device => {
        const typeMatch =
          selectedType.value === 'all' ||
          device.deviceType.toLowerCase() === selectedType.value.toLowerCase();
        const statusMatch =
          selectedStatus.value === 'all' ||
          device.status.toLowerCase() === selectedStatus.value.toLowerCase();

        if (!filterText.value) {
          return typeMatch && statusMatch;
        }

        const searchTerm = filterText.value.toLowerCase();
        const nameMatch = device.name.toLowerCase().includes(searchTerm);
        const idMatch = device.id.toLowerCase().includes(searchTerm);
        const modelMatch =
          device.model && device.model.toLowerCase().includes(searchTerm);

        return typeMatch && statusMatch && (nameMatch || idMatch || modelMatch);
      });
    });

    const filterApplied = computed(() => {
      return (
        filterText.value !== '' ||
        selectedType.value !== 'all' ||
        selectedStatus.value !== 'all'
      );
    });

    const paginatedDevices = computed(() => {
      const startIndex = (currentPage.value - 1) * props.itemsPerPage;
      const endIndex = startIndex + props.itemsPerPage;
      return filteredDevices.value.slice(startIndex, endIndex);
    });

    const totalPages = computed(() => {
      return Math.ceil(filteredDevices.value.length / props.itemsPerPage);
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

        // Add ownerId if provided
        if (props.ownerId) {
          params.ownerId = props.ownerId;
        }

        // Add network if provided
        if (props.network) {
          params.network = props.network;
        }

        // Fetch devices using the SDK
        const deviceData = await wrfcoinStore.sdk.getIoTDevices(params);

        if (deviceData && Array.isArray(deviceData)) {
          devices.value = deviceData;
        } else {
          // If we don't get data from the SDK, use mock data for demonstration
          devices.value = generateMockDevices();
        }

        // Set selected device if initialDeviceId is provided
        if (props.initialDeviceId) {
          const device = devices.value.find(
            d => d.id === props.initialDeviceId
          );
          if (device) {
            selectedDeviceId.value = device.id;
          }
        }

        emit('data-loaded', devices.value);
      } catch (err) {
        console.error('Error fetching devices:', err);
        error.value = err.message || 'Failed to fetch device data';
      } finally {
        loading.value = false;
      }
    };

    const generateMockDevices = () => {
      // Generate mock devices for demonstration purposes
      const types = [
        'WEATHER_STATION',
        'AIR_QUALITY_MONITOR',
        'SOIL_MOISTURE_SENSOR',
        'WATER_LEVEL_SENSOR',
        'GENERIC_SENSOR',
      ];
      const statuses = ['Active', 'Inactive', 'Maintenance', 'Error'];
      const mockDevices = [];

      // Generate random devices
      const deviceCount = 15; // Generate 15 devices

      for (let i = 0; i < deviceCount; i++) {
        const type = types[Math.floor(Math.random() * types.length)];
        const status = statuses[Math.floor(Math.random() * statuses.length)];

        // Randomize last reported time within the last day
        const lastReported = new Date();
        lastReported.setHours(
          lastReported.getHours() - Math.floor(Math.random() * 24)
        ); // Up to 24 hours ago

        const device = {
          id: `device-${Math.random().toString(36).substring(2, 10)}`,
          name: `${type.split('_')[0].charAt(0) + type.split('_')[0].slice(1).toLowerCase()} ${i + 1}`,
          deviceType: type,
          model: `Model ${Math.floor(Math.random() * 1000)}`,
          status: status,
          lastReported: lastReported.toISOString(),
        };

        // Add location to some devices
        if (Math.random() > 0.3) {
          device.location = {
            latitude: (Math.random() * 180 - 90).toFixed(6),
            longitude: (Math.random() * 360 - 180).toFixed(6),
            elevation: Math.floor(Math.random() * 1000),
          };
        }

        // Add capabilities based on device type
        if (type === 'WEATHER_STATION') {
          const parameters = [
            'temperature',
            'humidity',
            'pressure',
            'windSpeed',
            'windDirection',
            'rainfall',
          ];
          device.capabilities = {
            parameters: parameters.slice(
              0,
              Math.floor(Math.random() * parameters.length) + 1
            ),
            updateInterval: Math.floor(Math.random() * 30) + 5,
          };
        } else if (type === 'AIR_QUALITY_MONITOR') {
          const pollutants = ['pm25', 'pm10', 'o3', 'no2', 'so2', 'co'];
          device.capabilities = {
            pollutants: pollutants.slice(
              0,
              Math.floor(Math.random() * pollutants.length) + 1
            ),
          };
        }

        mockDevices.push(device);
      }

      return mockDevices;
    };

    const getDeviceColor = device => {
      // Assign different colors to different device types
      const typeColors = {
        WEATHER_STATION: '#3498db', // blue
        AIR_QUALITY_MONITOR: '#2ecc71', // green
        SOIL_MOISTURE_SENSOR: '#9b59b6', // purple
        WATER_LEVEL_SENSOR: '#e74c3c', // red
        GENERIC_SENSOR: '#f39c12', // orange
      };

      return typeColors[device.deviceType] || '#34495e'; // default color
    };

    const formatId = id => {
      if (!id) return '';

      // If ID is long, truncate it
      if (id.length > 16) {
        return `${id.substring(0, 8)}...${id.substring(id.length - 8)}`;
      }

      return id;
    };

    const formatTimestamp = timestamp => {
      try {
        if (!timestamp) return 'Unknown';

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

    const formatDeviceType = type => {
      if (!type) return 'Unknown';

      // Find the label from deviceTypes prop
      const typeObj = props.deviceTypes.find(
        t => t.value.toLowerCase() === type.toLowerCase()
      );
      if (typeObj) {
        return typeObj.label;
      }

      // Fallback: Replace underscores with spaces and capitalize each word
      return type
        .split('_')
        .map(word => word.charAt(0) + word.slice(1).toLowerCase())
        .join(' ');
    };

    const formatStatus = status => {
      if (!status) return 'Unknown';
      return status.charAt(0).toUpperCase() + status.slice(1).toLowerCase();
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

    const formatParameters = parameters => {
      if (
        !parameters ||
        !Array.isArray(parameters) ||
        parameters.length === 0
      ) {
        return '';
      }

      // Format parameters for display (capitalize and join with commas)
      return parameters
        .map(param => param.charAt(0).toUpperCase() + param.slice(1))
        .join(', ');
    };

    const isWeatherStation = device => {
      return (
        device.deviceType === 'WEATHER_STATION' &&
        device.capabilities &&
        Array.isArray(device.capabilities.parameters)
      );
    };

    const handleFilterChange = () => {
      currentPage.value = 1; // Reset to first page when filter changes
    };

    const handleTypeFilterChange = () => {
      currentPage.value = 1;
    };

    const handleStatusFilterChange = () => {
      currentPage.value = 1;
    };

    const clearAllFilters = () => {
      filterText.value = '';
      selectedType.value = 'all';
      selectedStatus.value = 'all';
      currentPage.value = 1;
    };

    const handleDeviceSelect = device => {
      selectedDeviceId.value = device.id;

      if (props.onDeviceSelect) {
        props.onDeviceSelect(device);
      }

      emit('device-selected', device);
    };

    const handleViewDetails = device => {
      if (props.onViewDetails) {
        props.onViewDetails(device);
      }

      emit('view-details', device);
    };

    const handleViewData = device => {
      if (props.onViewData) {
        props.onViewData(device);
      }

      emit('view-data', device);
    };

    const handleEditDevice = device => {
      if (props.onEditDevice) {
        props.onEditDevice(device);
      }

      emit('edit-device', device);
    };

    const handlePageChange = page => {
      if (typeof page === 'number') {
        currentPage.value = page;
        emit('page-change', page);
      }
    };

    const handleLiveUpdatesChange = () => {
      if (liveUpdates.value) {
        setupWebSocketListener();
      } else {
        removeWebSocketListener();
      }

      emit('live-updates-change', liveUpdates.value);
    };

    const refreshData = () => {
      fetchData();
    };

    // Setup auto-refresh
    const setupAutoRefresh = () => {
      clearInterval(intervalId.value);

      if (props.refreshInterval > 0 && !liveUpdates.value) {
        intervalId.value = setInterval(() => {
          fetchData();
        }, props.refreshInterval);
      }
    };

    // Setup WebSocket listener for live updates
    const setupWebSocketListener = () => {
      if (!wrfcoinStore.sdk || !wrfcoinStore.sdk.subscribeToDeviceUpdates) {
        console.warn('SDK does not support device update subscriptions');
        return;
      }

      removeWebSocketListener(); // Remove existing listener if any

      const params = {};
      if (props.ownerId) {
        params.ownerId = props.ownerId;
      }
      if (props.network) {
        params.network = props.network;
      }

      wsHandlerId.value = wrfcoinStore.sdk.subscribeToDeviceUpdates(
        deviceUpdate => {
          // Handle updates to existing devices
          if (deviceUpdate.action === 'update') {
            const index = devices.value.findIndex(
              d => d.id === deviceUpdate.device.id
            );
            if (index !== -1) {
              devices.value[index] = {
                ...devices.value[index],
                ...deviceUpdate.device,
              };
            }
          }
          // Handle new devices
          else if (deviceUpdate.action === 'add') {
            devices.value = [deviceUpdate.device, ...devices.value];
          }
          // Handle device removal
          else if (deviceUpdate.action === 'remove') {
            devices.value = devices.value.filter(
              d => d.id !== deviceUpdate.deviceId
            );
          }

          emit('device-update', deviceUpdate);
        },
        params
      );
    };

    // Remove WebSocket listener
    const removeWebSocketListener = () => {
      if (
        wsHandlerId.value &&
        wrfcoinStore.sdk &&
        wrfcoinStore.sdk.unsubscribeFromDeviceUpdates
      ) {
        wrfcoinStore.sdk.unsubscribeFromDeviceUpdates(wsHandlerId.value);
        wsHandlerId.value = null;
      }
    };

    // Watch for prop changes
    watch(
      () => props.ownerId,
      () => {
        fetchData();
        if (liveUpdates.value) {
          setupWebSocketListener();
        }
      }
    );

    watch(
      () => props.network,
      () => {
        fetchData();
        if (liveUpdates.value) {
          setupWebSocketListener();
        }
      }
    );

    // Lifecycle hooks
    onMounted(() => {
      fetchData();
      setupAutoRefresh();
      if (liveUpdates.value) {
        setupWebSocketListener();
      }
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
      devices,
      filteredDevices,
      paginatedDevices,
      loading,
      error,
      filterText,
      selectedType,
      selectedStatus,
      selectedDeviceId,
      currentPage,
      totalPages,
      visiblePageNumbers,
      liveUpdates,
      filterApplied,
      handleFilterChange,
      handleTypeFilterChange,
      handleStatusFilterChange,
      clearAllFilters,
      handleDeviceSelect,
      handleViewDetails,
      handleViewData,
      handleEditDevice,
      handlePageChange,
      handleLiveUpdatesChange,
      refreshData,
      getDeviceColor,
      formatId,
      formatTimestamp,
      formatDeviceType,
      formatStatus,
      formatLocation,
      formatParameters,
      isWeatherStation,
    };
  },
};
</script>

<style scoped>
.device-list {
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

.device-filter input {
  padding: 0.25rem 0.5rem;
  border-radius: 4px;
  border: 1px solid #ddd;
  font-size: 0.9rem;
  width: 200px;
}

.device-type-filter select,
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

.device-count {
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

.device-items {
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
}

.device-item {
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

.device-item:hover {
  background-color: #f8f8f8;
  border-color: #ddd;
}

.device-item.active {
  background-color: #f0f7ff;
  border-color: #3498db;
}

.device-icon {
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

.device-details {
  flex-grow: 1;
  overflow: hidden;
}

.device-id-row {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 0.25rem;
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

.device-type-id {
  display: flex;
  justify-content: space-between;
  font-size: 0.9rem;
  margin-bottom: 0.25rem;
}

.device-type {
  color: #666;
}

.device-id {
  font-family: monospace;
  color: #999;
  font-size: 0.8rem;
}

.device-location {
  font-size: 0.85rem;
  color: #666;
  margin-bottom: 0.25rem;
  display: flex;
  align-items: center;
  gap: 0.25rem;
}

.location-icon {
  font-style: normal;
  color: #3498db;
}

.device-footer {
  display: flex;
  justify-content: space-between;
  font-size: 0.8rem;
  color: #777;
}

.device-last-reported {
  flex-shrink: 0;
}

.device-parameters {
  color: #666;
  text-overflow: ellipsis;
  overflow: hidden;
  white-space: nowrap;
  max-width: 60%;
}

.device-actions {
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

.details-button {
  color: #95a5a6;
}

.data-button {
  color: #3498db;
}

.edit-button {
  color: #f39c12;
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

.live-updates-toggle {
  display: flex;
  justify-content: flex-end;
  margin-top: 1rem;
}

.toggle-label {
  display: flex;
  align-items: center;
  cursor: pointer;
}

.toggle-label input {
  margin-right: 0.5rem;
}

.toggle-text {
  font-size: 0.85rem;
  color: #666;
}

.loading-indicator,
.error-message,
.empty-devices {
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
