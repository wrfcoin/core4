import React, { createContext, useState, useContext, useEffect } from 'react';
import PropTypes from 'prop-types';
import { WrfcoinSDK } from '@wrfcoin/sdk';

// Create context
const WrfcoinContext = createContext(null);

/**
 * Wrfcoin Provider component for sharing Wrfcoin SDK and state across components
 *
 * This provider component initializes the Wrfcoin SDK and provides it to all child components,
 * along with shared state and utility functions.
 */
export const WrfcoinProvider = ({
  children,
  network = 'testnet',
  apiUrl,
  webSocketUrl,
  privateKey,
  storage,
  theme = 'light',
  cacheOptions = {
    enabled: true,
    ttl: 300000, // 5 minutes in milliseconds
  },
}) => {
  const [sdk, setSdk] = useState(null);
  const [blockchainStatus, setBlockchainStatus] = useState(null);
  const [wallet, setWallet] = useState(null);
  const [isConnected, setIsConnected] = useState(false);
  const [isInitializing, setIsInitializing] = useState(true);
  const [error, setError] = useState(null);

  // Initialize the SDK
  useEffect(() => {
    const initSDK = async () => {
      try {
        setIsInitializing(true);
        setError(null);

        const wrfcoinSdk = new WrfcoinSDK({
          network,
          apiUrl,
          webSocketUrl,
          privateKey,
          storage,
          cacheOptions,
        });

        // Initialize the SDK
        await wrfcoinSdk.initialize();

        setSdk(wrfcoinSdk);
        setIsInitializing(false);
      } catch (err) {
        console.error('Error initializing Wrfcoin SDK:', err);
        setError(
          'Failed to initialize Wrfcoin SDK. Please check your configuration.'
        );
        setIsInitializing(false);
      }
    };

    initSDK();

    // Clean up resources when the provider is unmounted
    return () => {
      if (sdk) {
        sdk.disconnect();
      }
    };
  }, [network, apiUrl, webSocketUrl, privateKey, storage]);

  // Get blockchain status periodically
  useEffect(() => {
    if (!sdk) return;

    const fetchBlockchainStatus = async () => {
      try {
        const status = await sdk.getBlockchainStatus();
        setBlockchainStatus(status);
        setIsConnected(true);
      } catch (err) {
        console.error('Error fetching blockchain status:', err);
        setIsConnected(false);
      }
    };

    // Fetch status immediately
    fetchBlockchainStatus();

    // Set up interval to fetch status periodically
    const interval = setInterval(fetchBlockchainStatus, 30000); // Every 30 seconds

    return () => clearInterval(interval);
  }, [sdk]);

  // Set up WebSocket connection for real-time updates
  useEffect(() => {
    if (!sdk || !sdk.connectWebSocket) return;

    const handleBlockchainUpdate = data => {
      if (data && data.state) {
        setBlockchainStatus(data.state);
      }
    };

    const handleConnectionStatus = connected => {
      setIsConnected(connected);
    };

    // Connect to WebSocket
    sdk.connectWebSocket({
      onBlockchainUpdate: handleBlockchainUpdate,
      onConnectionStatus: handleConnectionStatus,
    });

    return () => {
      sdk.disconnectWebSocket();
    };
  }, [sdk]);

  // Update wallet when privateKey changes
  useEffect(() => {
    if (!sdk) return;

    const updateWallet = async () => {
      if (privateKey) {
        try {
          const walletInfo = await sdk.getWalletInfo();
          setWallet(walletInfo);
        } catch (err) {
          console.error('Error getting wallet info:', err);
          setWallet(null);
        }
      } else {
        setWallet(null);
      }
    };

    updateWallet();
  }, [sdk, privateKey]);

  // Set private key in SDK
  const setPrivateKey = newPrivateKey => {
    if (sdk) {
      sdk.setPrivateKey(newPrivateKey);
    }
  };

  // Connect a wallet
  const connectWallet = async walletData => {
    if (!sdk) return null;

    try {
      const connectedWallet = await sdk.connectWallet(walletData);
      setWallet(connectedWallet);
      return connectedWallet;
    } catch (err) {
      console.error('Error connecting wallet:', err);
      throw err;
    }
  };

  // Disconnect the wallet
  const disconnectWallet = () => {
    if (!sdk) return;

    sdk.disconnectWallet();
    setWallet(null);
  };

  // Switch network
  const switchNetwork = newNetwork => {
    if (sdk) {
      sdk.setNetwork(newNetwork);
    }
  };

  // The context value
  const contextValue = {
    sdk,
    network,
    blockchainStatus,
    wallet,
    isConnected,
    isInitializing,
    error,
    theme,
    setPrivateKey,
    connectWallet,
    disconnectWallet,
    switchNetwork,
  };

  return (
    <WrfcoinContext.Provider value={contextValue}>
      {children}
    </WrfcoinContext.Provider>
  );
};

WrfcoinProvider.propTypes = {
  children: PropTypes.node.isRequired,
  network: PropTypes.oneOf(['mainnet', 'testnet']),
  apiUrl: PropTypes.string,
  webSocketUrl: PropTypes.string,
  privateKey: PropTypes.string,
  storage: PropTypes.object,
  theme: PropTypes.oneOf(['light', 'dark', 'system']),
  cacheOptions: PropTypes.shape({
    enabled: PropTypes.bool,
    ttl: PropTypes.number,
  }),
};

// Hook for using the Wrfcoin context
export const useWrfcoinContext = () => {
  const context = useContext(WrfcoinContext);

  if (!context) {
    throw new Error('useWrfcoinContext must be used within a WrfcoinProvider');
  }

  return context;
};
