<template>
  <div class="wallet-balance" :class="{ 'is-loading': loading }">
    <div v-if="error" class="error-message">
      {{ error }}
    </div>
    <div v-else class="balance-container">
      <div class="balance-header">{{ title }}</div>
      <div class="balance-amount">{{ formattedBalance }}</div>
      <div v-if="subtitle" class="balance-subtitle">{{ subtitle }}</div>
      <div
        v-if="showRefresh && !loading"
        class="refresh-button"
        @click="refreshBalance"
      >
        <slot name="refresh-icon"> ↻ </slot>
      </div>
      <div v-if="loading" class="loading-indicator">
        <slot name="loading-indicator"> Loading... </slot>
      </div>
    </div>
  </div>
</template>

<script>
import { ref, computed, onMounted, watch } from 'vue';
import { useWrfcoinStore } from '../../store/WrfcoinStore';

export default {
  name: 'WalletBalance',
  props: {
    address: {
      type: String,
      default: null, // Will use the current wallet from store if not provided
    },
    title: {
      type: String,
      default: 'Balance',
    },
    subtitle: {
      type: String,
      default: null,
    },
    refreshInterval: {
      type: Number,
      default: 0, // 0 means no auto-refresh
    },
    showRefresh: {
      type: Boolean,
      default: true,
    },
    formatOptions: {
      type: Object,
      default: () => ({
        style: 'decimal',
        minimumFractionDigits: 2,
        maximumFractionDigits: 6,
      }),
    },
    currencySymbol: {
      type: String,
      default: 'WRF',
    },
    symbolPosition: {
      type: String,
      default: 'after', // 'before' or 'after'
    },
  },
  setup(props, { emit }) {
    const wrfcoinStore = useWrfcoinStore();
    const balance = ref(0);
    const loading = ref(false);
    const error = ref(null);
    const intervalId = ref(null);

    // Computed property for formatted balance
    const formattedBalance = computed(() => {
      if (loading.value) {
        return '—';
      }

      try {
        const formatted = new Intl.NumberFormat(
          navigator.language || 'en-US',
          props.formatOptions
        ).format(balance.value);

        return props.symbolPosition === 'before'
          ? `${props.currencySymbol} ${formatted}`
          : `${formatted} ${props.currencySymbol}`;
      } catch (err) {
        console.error('Error formatting balance:', err);
        return `${balance.value} ${props.currencySymbol}`;
      }
    });

    // Function to fetch balance
    const fetchBalance = async () => {
      const walletAddress =
        props.address ||
        (wrfcoinStore.state.currentWallet &&
          wrfcoinStore.state.currentWallet.address);

      if (!walletAddress) {
        error.value = 'No wallet address provided';
        return;
      }

      try {
        loading.value = true;
        error.value = null;

        const newBalance = await wrfcoinStore.getWalletBalance(walletAddress);
        balance.value = newBalance;

        // Emit balance update event
        emit('balance-update', {
          address: walletAddress,
          balance: newBalance,
        });
      } catch (err) {
        console.error('Error fetching wallet balance:', err);
        error.value = err.message || 'Failed to fetch wallet balance';
      } finally {
        loading.value = false;
      }
    };

    // Function to manually refresh balance
    const refreshBalance = () => {
      fetchBalance();
    };

    // Setup auto-refresh if interval is provided
    const setupAutoRefresh = () => {
      clearInterval(intervalId.value);

      if (props.refreshInterval > 0) {
        intervalId.value = setInterval(() => {
          fetchBalance();
        }, props.refreshInterval);
      }
    };

    // Watch for address changes
    watch(
      () => props.address,
      () => {
        fetchBalance();
      }
    );

    // Watch for current wallet changes if no address is provided
    watch(
      () => wrfcoinStore.state.currentWallet,
      () => {
        if (!props.address) {
          fetchBalance();
        }
      },
      { deep: true }
    );

    // Lifecycle hooks
    onMounted(() => {
      fetchBalance();
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
      balance,
      loading,
      error,
      formattedBalance,
      refreshBalance,
    };
  },
};
</script>

<style scoped>
.wallet-balance {
  font-family:
    -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Helvetica, Arial,
    sans-serif;
  padding: 1rem;
  border-radius: 8px;
  background-color: #ffffff;
  box-shadow: 0 2px 4px rgba(0, 0, 0, 0.1);
  position: relative;
  transition: all 0.3s ease;
}

.wallet-balance.is-loading {
  opacity: 0.7;
}

.balance-header {
  font-size: 0.9rem;
  color: #666;
  margin-bottom: 0.5rem;
}

.balance-amount {
  font-size: 1.8rem;
  font-weight: 600;
  color: #333;
  margin-bottom: 0.25rem;
}

.balance-subtitle {
  font-size: 0.8rem;
  color: #888;
}

.error-message {
  color: #e74c3c;
  font-size: 0.9rem;
}

.refresh-button {
  position: absolute;
  top: 0.75rem;
  right: 0.75rem;
  cursor: pointer;
  color: #3498db;
  font-size: 1rem;
}

.loading-indicator {
  display: inline-block;
  margin-left: 0.5rem;
  font-size: 0.8rem;
  color: #7f8c8d;
}
</style>
