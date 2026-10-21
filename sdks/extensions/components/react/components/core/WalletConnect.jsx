import React, { useState, useEffect, useCallback } from 'react';
import PropTypes from 'prop-types';
import { WrfcoinSDK } from '@wrfcoin/sdk';
import { useWrfcoinContext } from '../../context/WrfcoinContext';

/**
 * WalletConnect component for connecting to Wrfcoin wallets
 *
 * This component provides a UI for connecting to different wallet types
 * and managing wallet connection state.
 */
const WalletConnect = ({
  onConnect,
  onDisconnect,
  theme = 'light',
  showBalance = true,
  showAddress = true,
  buttonStyle = {},
  modalStyle = {},
  modalPosition = 'center',
  supportedWallets = ['browser', 'hardware', 'mnemonic'],
  className = '',
  disconnectOnUnmount = false,
}) => {
  const [isOpen, setIsOpen] = useState(false);
  const [wallet, setWallet] = useState(null);
  const [walletType, setWalletType] = useState(null);
  const [balance, setBalance] = useState(null);
  const [error, setError] = useState(null);
  const [isLoading, setIsLoading] = useState(false);

  const { network, sdk = new WrfcoinSDK({ network: 'testnet' }) } =
    useWrfcoinContext();

  // Load wallet from localStorage on component mount
  useEffect(() => {
    const savedWallet = localStorage.getItem('wrfcoin_wallet');
    const savedWalletType = localStorage.getItem('wrfcoin_wallet_type');

    if (savedWallet && savedWalletType) {
      try {
        const walletData = JSON.parse(savedWallet);
        setWallet(walletData);
        setWalletType(savedWalletType);
        fetchBalance(walletData.address);
      } catch (err) {
        console.error('Error loading saved wallet:', err);
        clearWallet();
      }
    }
  }, []);

  // Clean up wallet on unmount if disconnectOnUnmount is true
  useEffect(() => {
    return () => {
      if (disconnectOnUnmount && wallet) {
        clearWallet();
      }
    };
  }, [disconnectOnUnmount, wallet]);

  // Fetch wallet balance
  const fetchBalance = useCallback(
    async address => {
      try {
        setIsLoading(true);
        const balanceData = await sdk.getBalance(address);
        setBalance(balanceData);
        setIsLoading(false);
      } catch (err) {
        console.error('Error fetching balance:', err);
        setError('Failed to fetch wallet balance');
        setIsLoading(false);
      }
    },
    [sdk]
  );

  // Connect to wallet
  const connectWallet = async type => {
    try {
      setIsLoading(true);
      setError(null);

      let walletData;

      switch (type) {
        case 'browser':
          walletData = await connectBrowserWallet();
          break;
        case 'hardware':
          walletData = await connectHardwareWallet();
          break;
        case 'mnemonic':
          walletData = await connectMnemonicWallet();
          break;
        default:
          throw new Error(`Unsupported wallet type: ${type}`);
      }

      if (walletData) {
        setWallet(walletData);
        setWalletType(type);

        // Save wallet data to localStorage
        localStorage.setItem('wrfcoin_wallet', JSON.stringify(walletData));
        localStorage.setItem('wrfcoin_wallet_type', type);

        // Fetch balance for the connected wallet
        fetchBalance(walletData.address);

        // Call onConnect callback if provided
        if (onConnect) {
          onConnect(walletData);
        }

        setIsOpen(false);
      }

      setIsLoading(false);
    } catch (err) {
      console.error('Error connecting wallet:', err);
      setError(`Failed to connect wallet: ${err.message}`);
      setIsLoading(false);
    }
  };

  // Connect to browser wallet (placeholder implementation)
  const connectBrowserWallet = async () => {
    // Check if browser wallet is available
    if (window.wrfcoin) {
      try {
        const accounts = await window.wrfcoin.request({
          method: 'eth_requestAccounts',
        });
        return {
          address: accounts[0],
          provider: window.wrfcoin,
        };
      } catch (err) {
        throw new Error(`Browser wallet error: ${err.message}`);
      }
    } else {
      throw new Error('No Wrfcoin wallet detected in your browser');
    }
  };

  // Connect to hardware wallet (placeholder implementation)
  const connectHardwareWallet = async () => {
    // In a real implementation, this would interface with hardware wallet libraries
    // For now, we'll show a mock dialog
    const mockAddress = `Wrf${Math.random().toString(36).substring(2, 15)}`;

    return {
      address: mockAddress,
      type: 'hardware',
    };
  };

  // Connect using mnemonic phrase (placeholder implementation)
  const connectMnemonicWallet = async () => {
    // This would normally prompt for a mnemonic phrase
    // For security reasons, we'll implement this as a separate modal component

    // Placeholder implementation
    const mockAddress = `Wrf${Math.random().toString(36).substring(2, 15)}`;

    return {
      address: mockAddress,
      type: 'mnemonic',
    };
  };

  // Disconnect wallet
  const disconnectWallet = () => {
    clearWallet();

    // Call onDisconnect callback if provided
    if (onDisconnect) {
      onDisconnect();
    }
  };

  // Clear wallet data
  const clearWallet = () => {
    setWallet(null);
    setWalletType(null);
    setBalance(null);
    localStorage.removeItem('wrfcoin_wallet');
    localStorage.removeItem('wrfcoin_wallet_type');
  };

  // Toggle modal visibility
  const toggleModal = () => {
    setIsOpen(!isOpen);
    setError(null);
  };

  // Format address for display
  const formatAddress = address => {
    if (!address) return '';
    return `${address.substring(0, 6)}...${address.substring(address.length - 4)}`;
  };

  // Format balance for display
  const formatBalance = balanceData => {
    if (!balanceData) return '0.00 WRF';
    return `${parseFloat(balanceData.formatted).toFixed(2)} WRF`;
  };

  return (
    <div className={`wrfcoin-wallet-connect ${className}`} data-theme={theme}>
      {wallet ? (
        <div className="wrfcoin-wallet-info">
          {showAddress && (
            <div className="wrfcoin-wallet-address">
              {formatAddress(wallet.address)}
            </div>
          )}

          {showBalance && (
            <div className="wrfcoin-wallet-balance">
              {isLoading ? 'Loading...' : formatBalance(balance)}
            </div>
          )}

          <button
            className="wrfcoin-disconnect-button"
            onClick={disconnectWallet}
            style={buttonStyle}
          >
            Disconnect
          </button>
        </div>
      ) : (
        <button
          className="wrfcoin-connect-button"
          onClick={toggleModal}
          style={buttonStyle}
        >
          Connect Wallet
        </button>
      )}

      {isOpen && (
        <div
          className={`wrfcoin-wallet-modal wrfcoin-modal-${modalPosition}`}
          style={modalStyle}
        >
          <div className="wrfcoin-wallet-modal-header">
            <h3>Connect Wallet</h3>
            <button className="wrfcoin-modal-close" onClick={toggleModal}>
              ×
            </button>
          </div>

          <div className="wrfcoin-wallet-modal-body">
            {error && <div className="wrfcoin-wallet-error">{error}</div>}

            <div className="wrfcoin-wallet-options">
              {supportedWallets.includes('browser') && (
                <button
                  className="wrfcoin-wallet-option"
                  onClick={() => connectWallet('browser')}
                  disabled={isLoading}
                >
                  <span className="wrfcoin-wallet-option-icon">🌐</span>
                  <span className="wrfcoin-wallet-option-name">
                    Browser Wallet
                  </span>
                </button>
              )}

              {supportedWallets.includes('hardware') && (
                <button
                  className="wrfcoin-wallet-option"
                  onClick={() => connectWallet('hardware')}
                  disabled={isLoading}
                >
                  <span className="wrfcoin-wallet-option-icon">🔒</span>
                  <span className="wrfcoin-wallet-option-name">
                    Hardware Wallet
                  </span>
                </button>
              )}

              {supportedWallets.includes('mnemonic') && (
                <button
                  className="wrfcoin-wallet-option"
                  onClick={() => connectWallet('mnemonic')}
                  disabled={isLoading}
                >
                  <span className="wrfcoin-wallet-option-icon">🔑</span>
                  <span className="wrfcoin-wallet-option-name">
                    Mnemonic Phrase
                  </span>
                </button>
              )}
            </div>

            {isLoading && (
              <div className="wrfcoin-wallet-loading">Connecting...</div>
            )}
          </div>
        </div>
      )}
    </div>
  );
};

WalletConnect.propTypes = {
  onConnect: PropTypes.func,
  onDisconnect: PropTypes.func,
  theme: PropTypes.oneOf(['light', 'dark', 'system']),
  showBalance: PropTypes.bool,
  showAddress: PropTypes.bool,
  buttonStyle: PropTypes.object,
  modalStyle: PropTypes.object,
  modalPosition: PropTypes.oneOf(['center', 'top', 'bottom', 'left', 'right']),
  supportedWallets: PropTypes.arrayOf(
    PropTypes.oneOf(['browser', 'hardware', 'mnemonic'])
  ),
  className: PropTypes.string,
  disconnectOnUnmount: PropTypes.bool,
};

export default WalletConnect;
