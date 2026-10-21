import { reactive, readonly } from 'vue';

/**
 * Wrfcoin Vue Store
 *
 * This store provides access to the Wrfcoin SDK and manages the state
 * for Wrfcoin-related components.
 */
const state = reactive({
  sdk: null,
  initialized: false,
  loading: false,
  error: null,
  network: null,
  currentWallet: null,
  wallets: [],
  transactions: [],
  blockchainInfo: {
    blockHeight: 0,
    hashrate: 0,
    difficulty: 0,
    pendingTransactions: 0,
    networkId: null,
    chainId: null,
    network: null,
  },
  websocket: {
    connected: false,
    reconnecting: false,
    error: null,
  },
});

// Actions to mutate state
const actions = {
  /**
   * Initialize the Wrfcoin SDK
   * @param {Object} sdk - Wrfcoin SDK instance
   * @param {Object} options - Configuration options
   */
  initializeSdk(sdk, options = {}) {
    try {
      state.loading = true;
      state.error = null;

      if (!sdk) {
        throw new Error('SDK instance is required');
      }

      // Set SDK and initialized state
      state.sdk = sdk;
      state.initialized = true;

      // Set up websocket connection if enabled
      if (options.enableWebsocket !== false) {
        this.initializeWebsocket();
      }

      // Fetch initial blockchain info
      if (options.fetchBlockchainInfo !== false) {
        this.fetchBlockchainInfo();
      }

      // Load wallets from local storage if available
      if (options.loadWallets !== false) {
        this.loadWallets();
      }

      // Set network
      if (options.network) {
        state.network = options.network;
      }
    } catch (error) {
      console.error('Error initializing Wrfcoin SDK:', error);
      state.error = error.message || 'Failed to initialize Wrfcoin SDK';
      state.initialized = false;
    } finally {
      state.loading = false;
    }
  },

  /**
   * Initialize WebSocket connection
   */
  initializeWebsocket() {
    try {
      if (!state.sdk || !state.initialized) {
        throw new Error(
          'SDK must be initialized before establishing WebSocket connection'
        );
      }

      state.websocket.connected = false;
      state.websocket.reconnecting = false;
      state.websocket.error = null;

      // Connect to WebSocket
      const websocketConnection = state.sdk.connectWebSocket({
        onConnect: () => {
          state.websocket.connected = true;
          state.websocket.reconnecting = false;
        },
        onDisconnect: () => {
          state.websocket.connected = false;
        },
        onReconnecting: () => {
          state.websocket.reconnecting = true;
        },
        onError: error => {
          state.websocket.error = error;
        },
        onBlockchainUpdate: blockchainInfo => {
          state.blockchainInfo = {
            ...state.blockchainInfo,
            ...blockchainInfo,
          };
        },
        onTransaction: transaction => {
          // Add transaction to the list if it's not already there
          if (!state.transactions.find(tx => tx.id === transaction.id)) {
            state.transactions = [transaction, ...state.transactions];
          }
        },
      });

      // Store WebSocket connection for later use
      state.websocket.connection = websocketConnection;
    } catch (error) {
      console.error('Error initializing WebSocket connection:', error);
      state.websocket.error =
        error.message || 'Failed to initialize WebSocket connection';
      state.websocket.connected = false;
    }
  },

  /**
   * Fetch blockchain info
   */
  async fetchBlockchainInfo() {
    try {
      if (!state.sdk || !state.initialized) {
        throw new Error(
          'SDK must be initialized before fetching blockchain info'
        );
      }

      const info = await state.sdk.getBlockchainInfo();

      if (info) {
        state.blockchainInfo = {
          ...state.blockchainInfo,
          ...info,
        };
      }
    } catch (error) {
      console.error('Error fetching blockchain info:', error);
    }
  },

  /**
   * Load wallets from local storage
   */
  loadWallets() {
    try {
      // Try to get wallets from local storage
      const storedWallets = localStorage.getItem('wrfcoin_wallets');

      if (storedWallets) {
        state.wallets = JSON.parse(storedWallets);

        // Set current wallet to the first wallet if available and no current wallet is set
        if (state.wallets.length > 0 && !state.currentWallet) {
          state.currentWallet = state.wallets[0];
        }
      }
    } catch (error) {
      console.error('Error loading wallets from local storage:', error);
    }
  },

  /**
   * Save wallets to local storage
   */
  saveWallets() {
    try {
      if (state.wallets && state.wallets.length > 0) {
        // Remove private keys before storing
        const walletsToStore = state.wallets.map(wallet => ({
          address: wallet.address,
          name: wallet.name,
          balance: wallet.balance,
          hasPrivateKey: !!wallet.privateKey,
        }));

        localStorage.setItem('wrfcoin_wallets', JSON.stringify(walletsToStore));
      }
    } catch (error) {
      console.error('Error saving wallets to local storage:', error);
    }
  },

  /**
   * Create a new wallet
   * @param {string} name - Wallet name
   * @returns {Object} - Created wallet
   */
  async createWallet(name = 'New Wallet') {
    try {
      if (!state.sdk || !state.initialized) {
        throw new Error('SDK must be initialized before creating a wallet');
      }

      const newWallet = await state.sdk.createWallet({ name });

      if (newWallet) {
        // Add wallet to list
        state.wallets = [...state.wallets, newWallet];

        // Set as current wallet
        state.currentWallet = newWallet;

        // Save to local storage
        this.saveWallets();

        return newWallet;
      }
    } catch (error) {
      console.error('Error creating wallet:', error);
      throw error;
    }
  },

  /**
   * Import a wallet using private key
   * @param {string} privateKey - Private key to import
   * @param {string} name - Wallet name
   * @returns {Object} - Imported wallet
   */
  async importWallet(privateKey, name = 'Imported Wallet') {
    try {
      if (!state.sdk || !state.initialized) {
        throw new Error('SDK must be initialized before importing a wallet');
      }

      if (!privateKey) {
        throw new Error('Private key is required');
      }

      const importedWallet = await state.sdk.importWallet({ privateKey, name });

      if (importedWallet) {
        // Add wallet to list
        state.wallets = [...state.wallets, importedWallet];

        // Set as current wallet
        state.currentWallet = importedWallet;

        // Save to local storage
        this.saveWallets();

        return importedWallet;
      }
    } catch (error) {
      console.error('Error importing wallet:', error);
      throw error;
    }
  },

  /**
   * Set current wallet
   * @param {string} address - Wallet address to set as current
   * @returns {Object|null} - Current wallet or null if not found
   */
  setCurrentWallet(address) {
    if (!address) {
      state.currentWallet = null;
      return null;
    }

    const wallet = state.wallets.find(w => w.address === address);

    if (wallet) {
      state.currentWallet = wallet;
      return wallet;
    }

    return null;
  },

  /**
   * Get wallet balance
   * @param {string} address - Wallet address
   * @returns {Promise<number>} - Wallet balance
   */
  async getWalletBalance(address) {
    try {
      if (!state.sdk || !state.initialized) {
        throw new Error(
          'SDK must be initialized before getting wallet balance'
        );
      }

      address = address || (state.currentWallet && state.currentWallet.address);

      if (!address) {
        throw new Error('Wallet address is required');
      }

      const balance = await state.sdk.getWalletBalance(address);

      // Update wallet balance
      const walletIndex = state.wallets.findIndex(w => w.address === address);

      if (walletIndex >= 0) {
        state.wallets[walletIndex] = {
          ...state.wallets[walletIndex],
          balance,
        };

        if (state.currentWallet && state.currentWallet.address === address) {
          state.currentWallet = state.wallets[walletIndex];
        }

        this.saveWallets();
      }

      return balance;
    } catch (error) {
      console.error('Error getting wallet balance:', error);
      throw error;
    }
  },

  /**
   * Get wallet transaction history
   * @param {string} address - Wallet address
   * @returns {Promise<Array>} - Transaction history
   */
  async getWalletTransactions(address) {
    try {
      if (!state.sdk || !state.initialized) {
        throw new Error(
          'SDK must be initialized before getting wallet transactions'
        );
      }

      address = address || (state.currentWallet && state.currentWallet.address);

      if (!address) {
        throw new Error('Wallet address is required');
      }

      const transactions = await state.sdk.getWalletTransactions(address);

      // Update transactions list
      if (transactions && Array.isArray(transactions)) {
        // Filter out duplicates
        const existingIds = new Set(state.transactions.map(tx => tx.id));
        const newTransactions = transactions.filter(
          tx => !existingIds.has(tx.id)
        );

        // Add new transactions
        if (newTransactions.length > 0) {
          state.transactions = [...newTransactions, ...state.transactions];
        }
      }

      return transactions;
    } catch (error) {
      console.error('Error getting wallet transactions:', error);
      throw error;
    }
  },

  /**
   * Send tokens to another address
   * @param {Object} params - Transfer parameters
   * @param {string} params.to - Recipient address
   * @param {number} params.amount - Amount to send
   * @param {string} params.from - Sender address (optional, uses current wallet if not provided)
   * @param {string} params.privateKey - Private key (optional if already stored)
   * @returns {Promise<Object>} - Transaction object
   */
  async sendTokens(params) {
    try {
      if (!state.sdk || !state.initialized) {
        throw new Error('SDK must be initialized before sending tokens');
      }

      if (!params.to) {
        throw new Error('Recipient address is required');
      }

      if (!params.amount || params.amount <= 0) {
        throw new Error('Amount must be greater than 0');
      }

      const fromAddress =
        params.from || (state.currentWallet && state.currentWallet.address);

      if (!fromAddress) {
        throw new Error('Sender address is required');
      }

      // Private key - either provided in params or stored in current wallet
      const privateKey =
        params.privateKey ||
        (state.currentWallet &&
          state.currentWallet.address === fromAddress &&
          state.currentWallet.privateKey);

      if (!privateKey) {
        throw new Error('Private key is required to sign transaction');
      }

      const transaction = await state.sdk.sendTokens({
        from: fromAddress,
        to: params.to,
        amount: params.amount,
        privateKey,
      });

      if (transaction && transaction.id) {
        // Add transaction to the list
        state.transactions = [transaction, ...state.transactions];

        // Update balance if possible
        this.getWalletBalance(fromAddress).catch(err => {
          console.warn('Error updating wallet balance after transaction:', err);
        });
      }

      return transaction;
    } catch (error) {
      console.error('Error sending tokens:', error);
      throw error;
    }
  },
};

// Create a readonly state to prevent direct state modification
const getReadonlyState = () => readonly(state);

// Export store getter and actions
export function useWrfcoinStore() {
  return {
    state: getReadonlyState(),
    ...actions,
  };
}

// Create a plugin for Vue app installation
export const WrfcoinPlugin = {
  install(app, options = {}) {
    // Create global properties for accessing store
    app.config.globalProperties.$wrfcoin = {
      ...actions,
      state: getReadonlyState(),
    };

    // Initialize SDK if provided in options
    if (options.sdk) {
      actions.initializeSdk(options.sdk, options);
    }

    // Initialize composable for components
    app.provide('wrfcoinStore', {
      state: getReadonlyState(),
      ...actions,
    });
  },
};

export default {
  WrfcoinPlugin,
  useWrfcoinStore,
};
