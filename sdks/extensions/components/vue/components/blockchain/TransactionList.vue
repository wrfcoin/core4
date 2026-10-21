<template>
  <div class="transaction-list">
    <div class="list-header">
      <h3 class="list-title">{{ title }}</h3>
      <div class="list-controls">
        <div v-if="showFilter" class="transaction-filter">
          <input
            type="text"
            v-model="filterText"
            placeholder="Filter transactions..."
            @input="handleFilterChange"
          />
        </div>

        <div v-if="showTypeFilter" class="transaction-type-filter">
          <select v-model="selectedType" @change="handleTypeFilterChange">
            <option value="all">All Types</option>
            <option
              v-for="type in transactionTypes"
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
            <option value="confirmed">Confirmed</option>
            <option value="pending">Pending</option>
            <option value="failed">Failed</option>
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

    <div v-else-if="loading && !transactions.length" class="loading-indicator">
      <slot name="loading-indicator"> Loading transaction data... </slot>
    </div>

    <div v-else-if="!filteredTransactions.length" class="empty-transactions">
      <slot name="empty-state">
        <p>No transactions found</p>
        <p v-if="filterApplied">Try adjusting your filter criteria</p>
      </slot>
    </div>

    <div v-else>
      <div class="transaction-count">
        Showing {{ filteredTransactions.length }} of
        {{ transactions.length }} transactions
      </div>

      <div class="filter-summary" v-if="filterApplied">
        <div class="filter-tags">
          <div v-if="selectedType !== 'all'" class="filter-tag">
            Type: {{ formatTransactionType(selectedType) }}
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

      <div class="transaction-items">
        <div
          v-for="transaction in paginatedTransactions"
          :key="transaction.id"
          class="transaction-item"
          :class="{ active: selectedTransactionId === transaction.id }"
          @click="handleTransactionSelect(transaction)"
        >
          <div
            class="transaction-icon"
            :style="{ backgroundColor: getTransactionColor(transaction) }"
          >
            {{ transaction.type.charAt(0).toUpperCase() }}
          </div>

          <div class="transaction-details">
            <div class="transaction-id-row">
              <div class="transaction-id">{{ formatId(transaction.id) }}</div>
              <div
                :class="
                  'transaction-status status-' +
                  transaction.status.toLowerCase()
                "
              >
                {{ transaction.status }}
              </div>
            </div>

            <div class="transaction-type-amount">
              <span class="transaction-type">{{
                formatTransactionType(transaction.type)
              }}</span>
              <span class="transaction-amount"
                >{{ formatAmount(transaction.amount) }} WRF</span
              >
            </div>

            <div class="transaction-footer">
              <div class="transaction-fee">
                Fee: {{ formatAmount(transaction.fee) }} WRF
              </div>
              <div v-if="transaction.timestamp" class="transaction-timestamp">
                {{ formatTimestamp(transaction.timestamp) }}
              </div>
            </div>
          </div>

          <div class="transaction-actions">
            <button
              v-if="onViewDetails"
              class="action-button details-button"
              @click.stop="handleViewDetails(transaction)"
              title="View Transaction Details"
            >
              <slot name="view-details-icon"> ℹ️ </slot>
            </button>

            <button
              v-if="onExplore"
              class="action-button explore-button"
              @click.stop="handleExplore(transaction)"
              title="Explore Transaction"
            >
              <slot name="explore-icon"> 🔍 </slot>
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
  name: 'TransactionList',
  props: {
    title: {
      type: String,
      default: 'Transactions',
    },
    transactionTypes: {
      type: Array,
      default: () => [
        { value: 'regular', label: 'Regular' },
        { value: 'computational', label: 'Computational' },
        { value: 'governance', label: 'Governance' },
        { value: 'nft', label: 'NFT' },
      ],
    },
    initialTransactionId: {
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
    address: {
      type: String,
      default: null, // If provided, show only transactions for this address
    },
    network: {
      type: String,
      default: null, // If null, uses the current connected network
    },
    onViewDetails: {
      type: Function,
      default: null,
    },
    onExplore: {
      type: Function,
      default: null,
    },
    onTransactionSelect: {
      type: Function,
      default: null,
    },
  },
  setup(props, { emit }) {
    const wrfcoinStore = useWrfcoinStore();
    const transactions = ref([]);
    const loading = ref(false);
    const error = ref(null);
    const filterText = ref('');
    const selectedType = ref('all');
    const selectedStatus = ref('all');
    const selectedTransactionId = ref(props.initialTransactionId);
    const currentPage = ref(1);
    const liveUpdates = ref(true);
    const intervalId = ref(null);
    const wsHandlerId = ref(null);

    // Computed properties
    const filteredTransactions = computed(() => {
      return transactions.value.filter(tx => {
        const typeMatch =
          selectedType.value === 'all' ||
          tx.type.toLowerCase() === selectedType.value.toLowerCase();
        const statusMatch =
          selectedStatus.value === 'all' ||
          tx.status.toLowerCase() === selectedStatus.value.toLowerCase();

        if (!filterText.value) {
          return typeMatch && statusMatch;
        }

        const searchTerm = filterText.value.toLowerCase();
        const idMatch = tx.id.toLowerCase().includes(searchTerm);
        const amountMatch = tx.amount.toString().includes(searchTerm);

        return typeMatch && statusMatch && (idMatch || amountMatch);
      });
    });

    const filterApplied = computed(() => {
      return (
        filterText.value !== '' ||
        selectedType.value !== 'all' ||
        selectedStatus.value !== 'all'
      );
    });

    const paginatedTransactions = computed(() => {
      const startIndex = (currentPage.value - 1) * props.itemsPerPage;
      const endIndex = startIndex + props.itemsPerPage;
      return filteredTransactions.value.slice(startIndex, endIndex);
    });

    const totalPages = computed(() => {
      return Math.ceil(filteredTransactions.value.length / props.itemsPerPage);
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

        // Add address if provided
        if (props.address) {
          params.address = props.address;
        }

        // Add network if provided
        if (props.network) {
          params.network = props.network;
        }

        // Fetch transactions using the SDK
        const transactionData = await wrfcoinStore.sdk.getTransactions(params);

        if (transactionData && Array.isArray(transactionData)) {
          transactions.value = transactionData;
        } else {
          // If we don't get data from the SDK, use mock data for demonstration
          transactions.value = generateMockTransactions();
        }

        // Set selected transaction if initialTransactionId is provided
        if (props.initialTransactionId) {
          const transaction = transactions.value.find(
            t => t.id === props.initialTransactionId
          );
          if (transaction) {
            selectedTransactionId.value = transaction.id;
          }
        }

        emit('data-loaded', transactions.value);
      } catch (err) {
        console.error('Error fetching transactions:', err);
        error.value = err.message || 'Failed to fetch transaction data';
      } finally {
        loading.value = false;
      }
    };

    const generateMockTransactions = () => {
      // Generate mock transactions for demonstration purposes
      const types = ['Regular', 'Computational', 'Governance', 'NFT'];
      const statuses = ['Confirmed', 'Pending', 'Failed'];
      const mockTransactions = [];

      // Generate random transactions
      const txCount = 25; // Generate 25 transactions

      for (let i = 0; i < txCount; i++) {
        const type = types[Math.floor(Math.random() * types.length)];
        const status = statuses[Math.floor(Math.random() * statuses.length)];

        // Randomize timestamp within the last day
        const timestamp = new Date();
        timestamp.setHours(
          timestamp.getHours() - Math.floor(Math.random() * 24)
        ); // Up to 24 hours ago

        // Random amount and fee
        const amount = Math.floor(Math.random() * 1000);
        const fee = (Math.random() * 0.1).toFixed(4);

        const tx = {
          id: `tx${Math.random().toString(36).substring(2, 10)}`,
          type: type,
          status: status,
          amount: amount,
          fee: fee,
          timestamp: timestamp.toISOString(),
        };

        mockTransactions.push(tx);
      }

      // Sort by timestamp, newest first
      return mockTransactions.sort((a, b) => {
        return new Date(b.timestamp) - new Date(a.timestamp);
      });
    };

    const getTransactionColor = transaction => {
      // Assign different colors to different transaction types
      const typeColors = {
        regular: '#3498db', // blue
        computational: '#2ecc71', // green
        governance: '#9b59b6', // purple
        nft: '#e74c3c', // red
      };

      return typeColors[transaction.type.toLowerCase()] || '#34495e'; // default color
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

    const formatTransactionType = type => {
      if (!type) return 'Unknown';

      // Find the label from transactionTypes prop
      const typeObj = props.transactionTypes.find(
        t => t.value.toLowerCase() === type.toLowerCase()
      );
      if (typeObj) {
        return typeObj.label;
      }

      // Fallback to capitalized type
      return type.charAt(0).toUpperCase() + type.slice(1);
    };

    const formatStatus = status => {
      if (!status) return 'Unknown';
      return status.charAt(0).toUpperCase() + status.slice(1);
    };

    const formatAmount = amount => {
      if (amount === null || amount === undefined) {
        return '0';
      }

      // Format as a number with thousand separators
      return Number(amount).toLocaleString(undefined, {
        minimumFractionDigits: 0,
        maximumFractionDigits: 4,
      });
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

    const handleTransactionSelect = transaction => {
      selectedTransactionId.value = transaction.id;

      if (props.onTransactionSelect) {
        props.onTransactionSelect(transaction);
      }

      emit('transaction-selected', transaction);
    };

    const handleViewDetails = transaction => {
      if (props.onViewDetails) {
        props.onViewDetails(transaction);
      }

      emit('view-details', transaction);
    };

    const handleExplore = transaction => {
      if (props.onExplore) {
        props.onExplore(transaction);
      }

      emit('explore', transaction);
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
      if (!wrfcoinStore.sdk || !wrfcoinStore.sdk.subscribeToTransactions) {
        console.warn('SDK does not support transaction subscriptions');
        return;
      }

      removeWebSocketListener(); // Remove existing listener if any

      const params = {};
      if (props.address) {
        params.address = props.address;
      }
      if (props.network) {
        params.network = props.network;
      }

      wsHandlerId.value = wrfcoinStore.sdk.subscribeToTransactions(
        transaction => {
          // Add the new transaction to the list
          transactions.value = [transaction, ...transactions.value];
          emit('transaction-received', transaction);
        },
        params
      );
    };

    // Remove WebSocket listener
    const removeWebSocketListener = () => {
      if (
        wsHandlerId.value &&
        wrfcoinStore.sdk &&
        wrfcoinStore.sdk.unsubscribeFromTransactions
      ) {
        wrfcoinStore.sdk.unsubscribeFromTransactions(wsHandlerId.value);
        wsHandlerId.value = null;
      }
    };

    // Watch for prop changes
    watch(
      () => props.address,
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
      transactions,
      filteredTransactions,
      paginatedTransactions,
      loading,
      error,
      filterText,
      selectedType,
      selectedStatus,
      selectedTransactionId,
      currentPage,
      totalPages,
      visiblePageNumbers,
      liveUpdates,
      filterApplied,
      handleFilterChange,
      handleTypeFilterChange,
      handleStatusFilterChange,
      clearAllFilters,
      handleTransactionSelect,
      handleViewDetails,
      handleExplore,
      handlePageChange,
      handleLiveUpdatesChange,
      refreshData,
      getTransactionColor,
      formatId,
      formatTimestamp,
      formatTransactionType,
      formatStatus,
      formatAmount,
    };
  },
};
</script>

<style scoped>
.transaction-list {
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

.transaction-filter input {
  padding: 0.25rem 0.5rem;
  border-radius: 4px;
  border: 1px solid #ddd;
  font-size: 0.9rem;
  width: 200px;
}

.transaction-type-filter select,
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

.transaction-count {
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

.transaction-items {
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
}

.transaction-item {
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

.transaction-item:hover {
  background-color: #f8f8f8;
  border-color: #ddd;
}

.transaction-item.active {
  background-color: #f0f7ff;
  border-color: #3498db;
}

.transaction-icon {
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

.transaction-details {
  flex-grow: 1;
  overflow: hidden;
}

.transaction-id-row {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 0.25rem;
}

.transaction-id {
  font-family: monospace;
  font-size: 0.9rem;
  color: #666;
}

.transaction-status {
  font-size: 0.8rem;
  padding: 0.1rem 0.4rem;
  border-radius: 10px;
  white-space: nowrap;
}

.status-confirmed {
  background-color: #e8f7f0;
  color: #2ecc71;
}

.status-pending {
  background-color: #fef8e8;
  color: #f39c12;
}

.status-failed {
  background-color: #feeaea;
  color: #e74c3c;
}

.transaction-type-amount {
  font-weight: 500;
  font-size: 1rem;
  margin-bottom: 0.25rem;
  display: flex;
  justify-content: space-between;
}

.transaction-footer {
  display: flex;
  justify-content: space-between;
  font-size: 0.8rem;
  color: #777;
}

.transaction-actions {
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

.explore-button {
  color: #3498db;
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
.empty-transactions {
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
