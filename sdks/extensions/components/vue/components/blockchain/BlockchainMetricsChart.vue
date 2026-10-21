<template>
  <div class="blockchain-metrics-chart">
    <div class="chart-header">
      <h3 class="chart-title">{{ title }}</h3>
      <div class="chart-controls">
        <div v-if="showMetricSelector" class="metric-selector">
          <label for="metric-select">Metric:</label>
          <select
            id="metric-select"
            v-model="selectedMetric"
            @change="handleMetricChange"
          >
            <option
              v-for="metric in availableMetrics"
              :key="metric.value"
              :value="metric.value"
            >
              {{ metric.label }}
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
      <slot name="loading-indicator"> Loading blockchain data... </slot>
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
            <p>{{ data.length }} data points for {{ selectedMetric }}.</p>
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

      <div v-if="showDataSource" class="data-source">
        <div class="source-label">Source:</div>
        <div class="source-value">{{ dataSource }}</div>
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
  name: 'BlockchainMetricsChart',
  props: {
    title: {
      type: String,
      default: 'Blockchain Metrics',
    },
    metrics: {
      type: Array,
      default: () => [
        { value: 'blockHeight', label: 'Block Height' },
        { value: 'hashrate', label: 'Hashrate' },
        { value: 'difficulty', label: 'Difficulty' },
        { value: 'pendingTransactions', label: 'Pending Transactions' },
        { value: 'transactionsPerSecond', label: 'Transactions Per Second' },
      ],
    },
    defaultMetric: {
      type: String,
      default: 'blockHeight',
    },
    showMetricSelector: {
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
    showDataSource: {
      type: Boolean,
      default: true,
    },
    dataSource: {
      type: String,
      default: 'Wrfcoin Network',
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
    const loading = ref(false);
    const error = ref(null);
    const selectedMetric = ref(props.defaultMetric);
    const selectedTimeRange = ref(props.defaultTimeRange);
    const graphRendered = ref(false);
    const lastUpdated = ref(null);
    const intervalId = ref(null);

    // Computed properties
    const availableMetrics = computed(() => {
      return props.metrics;
    });

    const currentValue = computed(() => {
      if (!data.value.length) {
        return null;
      }
      return data.value[data.value.length - 1][selectedMetric.value];
    });

    const averageValue = computed(() => {
      if (!data.value.length) {
        return null;
      }

      const sum = data.value.reduce((acc, item) => {
        return acc + (item[selectedMetric.value] || 0);
      }, 0);

      return sum / data.value.length;
    });

    const minValue = computed(() => {
      if (!data.value.length) {
        return null;
      }

      return Math.min(
        ...data.value.map(item => item[selectedMetric.value] || 0)
      );
    });

    const maxValue = computed(() => {
      if (!data.value.length) {
        return null;
      }

      return Math.max(
        ...data.value.map(item => item[selectedMetric.value] || 0)
      );
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
        const params = {
          metric: selectedMetric.value,
          timeRange: selectedTimeRange.value,
        };

        // Add network if provided
        if (props.network) {
          params.network = props.network;
        }

        // Fetch blockchain metrics using the SDK
        const metricsData = await wrfcoinStore.sdk.getBlockchainMetrics(params);

        if (metricsData && Array.isArray(metricsData)) {
          data.value = metricsData;
        } else {
          // If we don't get data from the SDK, use mock data for demonstration
          data.value = generateMockData();
        }

        lastUpdated.value = new Date();
        await nextTick();
        renderGraph();

        emit('data-loaded', data.value);
      } catch (err) {
        console.error('Error fetching blockchain metrics:', err);
        error.value = err.message || 'Failed to fetch blockchain metrics';
      } finally {
        loading.value = false;
      }
    };

    const generateMockData = () => {
      // Generate mock data for demonstration purposes
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

      // Generate data based on selected metric
      for (let i = 0; i < dataPoints; i++) {
        const timestamp = new Date(now.getTime() - (dataPoints - i) * timeStep);
        const point = { timestamp };

        // Add all metrics to data point
        point.blockHeight = 1000 + i * 6; // Steadily increasing blocks
        point.hashrate = 800 + Math.random() * 400; // 800-1200 range
        point.difficulty = 5 + Math.sin(i / 10) * 2; // 3-7 range, wavy pattern
        point.pendingTransactions = Math.floor(Math.random() * 20); // 0-20 range
        point.transactionsPerSecond = 5 + Math.random() * 10; // 5-15 range

        mockData.push(point);
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

      // Get the data values for the selected metric
      const metricValues = data.value.map(
        item => item[selectedMetric.value] || 0
      );
      const minVal = Math.min(...metricValues);
      const maxVal = Math.max(...metricValues);
      const valueRange = maxVal - minVal;

      metricValues.forEach((value, index) => {
        // Calculate percentage height (with a 5% minimum for visibility)
        const percentage =
          valueRange > 0 ? 5 + ((value - minVal) / valueRange) * 95 : 50;

        const bar = document.createElement('div');
        bar.style.height = `${percentage}%`;
        bar.style.width = `${100 / metricValues.length}%`;
        bar.style.backgroundColor = getColorForMetric(selectedMetric.value);
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

    const getColorForMetric = metric => {
      // Assign different colors to different metrics
      const colors = {
        blockHeight: '#3498db', // blue
        hashrate: '#2ecc71', // green
        difficulty: '#9b59b6', // purple
        pendingTransactions: '#e74c3c', // red
        transactionsPerSecond: '#f39c12', // orange
      };

      return colors[metric] || '#34495e'; // default color
    };

    const formatValue = value => {
      if (value === null || value === undefined) {
        return '—';
      }

      // Format based on selected metric
      switch (selectedMetric.value) {
        case 'blockHeight':
          return value.toLocaleString();
        case 'hashrate':
          return `${value.toFixed(0)} H/s`;
        case 'difficulty':
          return value.toFixed(2);
        case 'pendingTransactions':
          return value.toLocaleString();
        case 'transactionsPerSecond':
          return `${value.toFixed(2)} tx/s`;
        default:
          return value.toString();
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

    const handleMetricChange = () => {
      fetchData();
      emit('metric-change', selectedMetric.value);
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

    // Watch for prop changes
    watch(
      () => props.network,
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
      graphContainer,
      data,
      loading,
      error,
      selectedMetric,
      selectedTimeRange,
      graphRendered,
      lastUpdated,
      availableMetrics,
      currentValue,
      averageValue,
      minValue,
      maxValue,
      handleMetricChange,
      handleTimeRangeChange,
      refreshData,
      formatValue,
      formatTimestamp,
    };
  },
};
</script>

<style scoped>
.blockchain-metrics-chart {
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

.metric-selector select {
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

.data-source {
  display: flex;
  align-items: center;
  font-size: 0.8rem;
  color: #777;
  gap: 0.5rem;
  padding-top: 0.5rem;
  border-top: 1px solid #eee;
}

.source-label {
  font-weight: 500;
}

.last-updated {
  margin-left: auto;
}

.loading-indicator,
.error-message {
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
