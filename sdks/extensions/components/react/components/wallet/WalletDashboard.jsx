import React, { useState, useEffect } from 'react';
import PropTypes from 'prop-types';
import { useWrfcoinContext } from '../../context/WrfcoinContext';
import WalletBalance from './WalletBalance';
import WalletHistory from './WalletHistory';
import WalletTransfer from './WalletTransfer';

/**
 * WalletDashboard component for providing a complete wallet management interface
 *
 * This component combines various wallet components into a cohesive dashboard
 * with tabs for different wallet functions.
 */
const WalletDashboard = ({
  walletAddress,
  initialTab = 'balance',
  theme = 'light',
  refreshInterval = 10000,
}) => {
  const { sdk } = useWrfcoinContext();
  const [activeTab, setActiveTab] = useState(initialTab);
  const [walletInfo, setWalletInfo] = useState(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(null);
  const [showTransferForm, setShowTransferForm] = useState(false);
  const [selectedTransaction, setSelectedTransaction] = useState(null);

  // Tabs configuration
  const tabs = [
    { id: 'balance', label: 'Balance' },
    { id: 'history', label: 'History' },
    { id: 'assets', label: 'Assets' },
  ];

  // Fetch wallet info
  useEffect(() => {
    if (!sdk || !walletAddress) return;

    const fetchWalletInfo = async () => {
      try {
        setLoading(true);
        setError(null);

        const info = await sdk.getWalletInfo(walletAddress);
        setWalletInfo(info);
      } catch (err) {
        console.error('Error fetching wallet data:', err);
        setError('Failed to load wallet information. Please try again later.');
      } finally {
        setLoading(false);
      }
    };

    fetchWalletInfo();

    // Set up interval to refresh data
    const interval = setInterval(fetchWalletInfo, refreshInterval);

    return () => clearInterval(interval);
  }, [sdk, walletAddress, refreshInterval]);

  // Handle tab change
  const handleTabChange = tabId => {
    setActiveTab(tabId);
  };

  // Handle transfer click
  const handleTransferClick = () => {
    setShowTransferForm(true);
  };

  // Handle transfer cancel
  const handleTransferCancel = () => {
    setShowTransferForm(false);
  };

  // Handle transfer success
  const handleTransferSuccess = transaction => {
    setShowTransferForm(false);
    // You could add a success notification here if needed
  };

  // Handle transaction click
  const handleTransactionClick = transaction => {
    setSelectedTransaction(transaction);
  };

  // Close transaction detail
  const closeTransactionDetail = () => {
    setSelectedTransaction(null);
  };

  // Format transaction amount
  const formatTransactionAmount = transaction => {
    if (transaction.amount === undefined || transaction.amount === null)
      return 'N/A';

    // Determine if this is an incoming or outgoing transaction
    const isOutgoing = transaction.from === walletAddress;
    const prefix = isOutgoing ? '-' : '+';
    const amount = parseFloat(transaction.amount);

    return `${prefix} ${amount.toLocaleString(undefined, { minimumFractionDigits: 2, maximumFractionDigits: 6 })} WRF`;
  };

  // Format fee
  const formatFee = fee => {
    if (fee === undefined || fee === null) return 'N/A';
    return `${parseFloat(fee).toLocaleString(undefined, { minimumFractionDigits: 6, maximumFractionDigits: 6 })} WRF`;
  };

  // Format timestamp
  const formatTimestamp = timestamp => {
    if (!timestamp) return '';

    const date = new Date(timestamp);
    return date.toLocaleString();
  };

  // Get transaction type display name
  const getTransactionTypeDisplay = type => {
    switch (type) {
      case 'Regular':
        return 'Transfer';
      case 'NFT':
        return 'NFT';
      case 'Computational':
        return 'Compute';
      case 'Governance':
        return 'Gov';
      case 'Validator':
        return 'Validator';
      case 'StationRegistration':
        return 'Station';
      case 'WeatherData':
        return 'Weather';
      case 'ForecastData':
        return 'Forecast';
      default:
        return type;
    }
  };

  // Generate CSS for the component
  const styles = {
    container: {
      backgroundColor: theme === 'dark' ? '#333' : '#fff',
      color: theme === 'dark' ? '#fff' : '#333',
      border: theme === 'dark' ? '1px solid #444' : '1px solid #ddd',
      borderRadius: '8px',
      overflow: 'hidden',
      maxWidth: '100%',
    },
    header: {
      padding: '16px',
      borderBottom: theme === 'dark' ? '1px solid #444' : '1px solid #ddd',
      display: 'flex',
      justifyContent: 'space-between',
      alignItems: 'center',
      backgroundColor: theme === 'dark' ? '#2a2a2a' : '#f9f9f9',
    },
    title: {
      fontSize: '18px',
      fontWeight: '600',
    },
    walletAddress: {
      fontSize: '14px',
      color: theme === 'dark' ? '#aaa' : '#777',
      marginTop: '4px',
      wordBreak: 'break-all',
    },
    actions: {
      display: 'flex',
      gap: '8px',
    },
    actionButton: {
      padding: '6px 12px',
      backgroundColor: '#3a8eff',
      color: '#fff',
      border: 'none',
      borderRadius: '4px',
      cursor: 'pointer',
      fontSize: '14px',
      fontWeight: '500',
      transition: 'background-color 0.2s ease',
      '&:hover': {
        backgroundColor: '#2a7ae9',
      },
    },
    tabs: {
      display: 'flex',
      borderBottom: theme === 'dark' ? '1px solid #444' : '1px solid #ddd',
      backgroundColor: theme === 'dark' ? '#2a2a2a' : '#f9f9f9',
    },
    tab: isActive => ({
      padding: '12px 16px',
      fontSize: '14px',
      fontWeight: isActive ? '600' : '400',
      cursor: 'pointer',
      color: isActive
        ? theme === 'dark'
          ? '#fff'
          : '#333'
        : theme === 'dark'
          ? '#aaa'
          : '#777',
      borderBottom: isActive
        ? '2px solid #3a8eff'
        : theme === 'dark'
          ? '2px solid #2a2a2a'
          : '2px solid #f9f9f9',
      transition: 'all 0.2s ease',
    }),
    content: {
      padding: '16px',
    },
    transactionDetailOverlay: {
      position: 'fixed',
      top: 0,
      left: 0,
      right: 0,
      bottom: 0,
      backgroundColor: 'rgba(0, 0, 0, 0.5)',
      display: 'flex',
      justifyContent: 'center',
      alignItems: 'center',
      zIndex: 1000,
    },
    transactionDetail: {
      backgroundColor: theme === 'dark' ? '#333' : '#fff',
      border: theme === 'dark' ? '1px solid #444' : '1px solid #ddd',
      borderRadius: '8px',
      padding: '16px',
      width: '90%',
      maxWidth: '500px',
      maxHeight: '80vh',
      overflowY: 'auto',
    },
    transactionDetailHeader: {
      display: 'flex',
      justifyContent: 'space-between',
      alignItems: 'center',
      marginBottom: '16px',
    },
    transactionDetailTitle: {
      fontSize: '18px',
      fontWeight: '600',
    },
    closeButton: {
      backgroundColor: 'transparent',
      border: 'none',
      color: theme === 'dark' ? '#fff' : '#333',
      fontSize: '18px',
      cursor: 'pointer',
    },
    transactionDetailContent: {
      display: 'flex',
      flexDirection: 'column',
      gap: '12px',
    },
    transactionDetailRow: {
      display: 'flex',
      justifyContent: 'space-between',
    },
    transactionDetailLabel: {
      fontSize: '14px',
      color: theme === 'dark' ? '#aaa' : '#777',
    },
    transactionDetailValue: {
      fontSize: '14px',
      fontWeight: '500',
    },
    transactionDetailAmount: transaction => ({
      fontSize: '14px',
      fontWeight: '500',
      color:
        transaction && transaction.from === walletAddress
          ? theme === 'dark'
            ? '#ff8a8a'
            : '#cc0000'
          : theme === 'dark'
            ? '#8aff8a'
            : '#008000',
    }),
    transactionTypeTag: type => ({
      display: 'inline-block',
      padding: '2px 6px',
      borderRadius: '4px',
      fontSize: '12px',
      backgroundColor: getTypeColor(type, theme),
      color: '#fff',
    }),
    loading: {
      padding: '16px',
      textAlign: 'center',
      color: theme === 'dark' ? '#aaa' : '#777',
    },
    error: {
      padding: '16px',
      textAlign: 'center',
      color: theme === 'dark' ? '#ff8a8a' : '#cc0000',
    },
    assetList: {
      display: 'flex',
      flexDirection: 'column',
      gap: '12px',
    },
    assetItem: {
      padding: '12px',
      borderRadius: '4px',
      backgroundColor: theme === 'dark' ? '#222' : '#f9f9f9',
      display: 'flex',
      justifyContent: 'space-between',
      alignItems: 'center',
    },
    assetInfo: {
      display: 'flex',
      alignItems: 'center',
      gap: '8px',
    },
    assetIcon: {
      width: '32px',
      height: '32px',
      borderRadius: '16px',
      backgroundColor: theme === 'dark' ? '#444' : '#ddd',
      display: 'flex',
      justifyContent: 'center',
      alignItems: 'center',
      fontSize: '14px',
      fontWeight: '500',
    },
    assetDetails: {
      display: 'flex',
      flexDirection: 'column',
    },
    assetName: {
      fontSize: '14px',
      fontWeight: '500',
    },
    assetType: {
      fontSize: '12px',
      color: theme === 'dark' ? '#aaa' : '#777',
    },
    assetValue: {
      fontSize: '14px',
      fontWeight: '500',
    },
  };

  // Get color for transaction type tag
  function getTypeColor(type, themeMode) {
    switch (type) {
      case 'Regular':
        return '#3a8eff';
      case 'NFT':
        return '#a86bff';
      case 'Computational':
        return '#00a69c';
      case 'Governance':
        return '#f67e2c';
      case 'Validator':
        return '#ffcc29';
      case 'StationRegistration':
        return '#5d6e85';
      case 'WeatherData':
        return '#6dbf45';
      case 'ForecastData':
        return '#a94ad5';
      default:
        return themeMode === 'dark' ? '#555' : '#999';
    }
  }

  // Dummy NFT data for the assets tab
  const dummyNFTs = [
    {
      id: 'nft1',
      name: 'Weather Station #103',
      type: 'Station NFT',
      value: '1 NFT',
    },
    {
      id: 'nft2',
      name: 'Climate Data Compute #27',
      type: 'Computational NFT',
      value: '1 NFT',
    },
  ];

  // Render transaction detail modal
  const renderTransactionDetail = () => {
    if (!selectedTransaction) return null;

    return (
      <div
        style={styles.transactionDetailOverlay}
        onClick={closeTransactionDetail}
      >
        <div
          style={styles.transactionDetail}
          onClick={e => e.stopPropagation()}
        >
          <div style={styles.transactionDetailHeader}>
            <div style={styles.transactionDetailTitle}>Transaction Details</div>
            <button style={styles.closeButton} onClick={closeTransactionDetail}>
              ×
            </button>
          </div>

          <div style={styles.transactionDetailContent}>
            <div style={styles.transactionDetailRow}>
              <div style={styles.transactionDetailLabel}>Transaction ID</div>
              <div style={styles.transactionDetailValue}>
                {selectedTransaction.id}
              </div>
            </div>

            <div style={styles.transactionDetailRow}>
              <div style={styles.transactionDetailLabel}>Type</div>
              <div style={styles.transactionDetailValue}>
                <span
                  style={styles.transactionTypeTag(selectedTransaction.type)}
                >
                  {getTransactionTypeDisplay(selectedTransaction.type)}
                </span>
              </div>
            </div>

            <div style={styles.transactionDetailRow}>
              <div style={styles.transactionDetailLabel}>Amount</div>
              <div style={styles.transactionDetailAmount(selectedTransaction)}>
                {formatTransactionAmount(selectedTransaction)}
              </div>
            </div>

            <div style={styles.transactionDetailRow}>
              <div style={styles.transactionDetailLabel}>Fee</div>
              <div style={styles.transactionDetailValue}>
                {formatFee(selectedTransaction.fee)}
              </div>
            </div>

            <div style={styles.transactionDetailRow}>
              <div style={styles.transactionDetailLabel}>Date & Time</div>
              <div style={styles.transactionDetailValue}>
                {formatTimestamp(selectedTransaction.timestamp)}
              </div>
            </div>

            {selectedTransaction.from && (
              <div style={styles.transactionDetailRow}>
                <div style={styles.transactionDetailLabel}>From</div>
                <div style={styles.transactionDetailValue}>
                  {selectedTransaction.from}
                </div>
              </div>
            )}

            {selectedTransaction.to && (
              <div style={styles.transactionDetailRow}>
                <div style={styles.transactionDetailLabel}>To</div>
                <div style={styles.transactionDetailValue}>
                  {selectedTransaction.to}
                </div>
              </div>
            )}

            {selectedTransaction.status && (
              <div style={styles.transactionDetailRow}>
                <div style={styles.transactionDetailLabel}>Status</div>
                <div style={styles.transactionDetailValue}>
                  {selectedTransaction.status}
                </div>
              </div>
            )}

            {selectedTransaction.memo && (
              <div style={styles.transactionDetailRow}>
                <div style={styles.transactionDetailLabel}>Memo</div>
                <div style={styles.transactionDetailValue}>
                  {selectedTransaction.memo}
                </div>
              </div>
            )}
          </div>
        </div>
      </div>
    );
  };

  return (
    <div style={styles.container} className="wallet-dashboard">
      <div style={styles.header}>
        <div>
          <div style={styles.title}>Wallet Dashboard</div>
          <div style={styles.walletAddress}>{walletAddress}</div>
        </div>

        <div style={styles.actions}>
          <button style={styles.actionButton} onClick={handleTransferClick}>
            Transfer
          </button>
        </div>
      </div>

      <div style={styles.tabs}>
        {tabs.map(tab => (
          <div
            key={tab.id}
            style={styles.tab(activeTab === tab.id)}
            onClick={() => handleTabChange(tab.id)}
          >
            {tab.label}
          </div>
        ))}
      </div>

      <div style={styles.content}>
        {loading ? (
          <div style={styles.loading}>Loading wallet information...</div>
        ) : error ? (
          <div style={styles.error}>{error}</div>
        ) : (
          <>
            {activeTab === 'balance' && !showTransferForm && (
              <WalletBalance
                walletAddress={walletAddress}
                showTransactions={true}
                theme={theme}
                onTransactionClick={handleTransactionClick}
                onTransferClick={handleTransferClick}
              />
            )}

            {activeTab === 'history' && !showTransferForm && (
              <WalletHistory
                walletAddress={walletAddress}
                showChart={true}
                showTypeFilter={true}
                theme={theme}
                onTransactionClick={handleTransactionClick}
              />
            )}

            {activeTab === 'assets' && !showTransferForm && (
              <div style={styles.assetList}>
                {dummyNFTs.map(nft => (
                  <div key={nft.id} style={styles.assetItem}>
                    <div style={styles.assetInfo}>
                      <div style={styles.assetIcon}>{nft.name.charAt(0)}</div>
                      <div style={styles.assetDetails}>
                        <div style={styles.assetName}>{nft.name}</div>
                        <div style={styles.assetType}>{nft.type}</div>
                      </div>
                    </div>
                    <div style={styles.assetValue}>{nft.value}</div>
                  </div>
                ))}
              </div>
            )}

            {showTransferForm && (
              <WalletTransfer
                walletAddress={walletAddress}
                theme={theme}
                onSuccess={handleTransferSuccess}
                onCancel={handleTransferCancel}
              />
            )}
          </>
        )}
      </div>

      {selectedTransaction && renderTransactionDetail()}
    </div>
  );
};

WalletDashboard.propTypes = {
  walletAddress: PropTypes.string.isRequired,
  initialTab: PropTypes.oneOf(['balance', 'history', 'assets']),
  theme: PropTypes.oneOf(['light', 'dark']),
  refreshInterval: PropTypes.number,
};

export default WalletDashboard;
