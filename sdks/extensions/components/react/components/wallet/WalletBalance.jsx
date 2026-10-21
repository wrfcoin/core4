import React, { useState, useEffect } from 'react';
import PropTypes from 'prop-types';
import { useWrfcoinContext } from '../../context/WrfcoinContext';

/**
 * WalletBalance component for displaying wallet balance and transaction history
 *
 * This component shows the current balance of a wallet and recent transactions,
 * with options to view the transaction history or transfer funds.
 */
const WalletBalance = ({
  walletAddress,
  showTransactions = true,
  maxTransactions = 5,
  showTransfer = true,
  theme = 'light',
  refreshInterval = 10000,
  onTransactionClick,
  onTransferClick,
}) => {
  const { sdk } = useWrfcoinContext();
  const [walletInfo, setWalletInfo] = useState(null);
  const [transactions, setTransactions] = useState([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(null);

  // Fetch wallet info and transactions
  useEffect(() => {
    if (!sdk || !walletAddress) return;

    const fetchWalletData = async () => {
      try {
        setLoading(true);
        setError(null);

        // Fetch wallet info
        const info = await sdk.getWalletInfo(walletAddress);
        setWalletInfo(info);

        // Fetch wallet transactions
        if (showTransactions) {
          const txData = await sdk.getTransactions({
            wallet: walletAddress,
            limit: maxTransactions,
          });
          setTransactions(txData);
        }
      } catch (err) {
        console.error('Error fetching wallet data:', err);
        setError('Failed to load wallet information. Please try again later.');
      } finally {
        setLoading(false);
      }
    };

    fetchWalletData();

    // Set up interval to refresh data
    const interval = setInterval(fetchWalletData, refreshInterval);

    return () => clearInterval(interval);
  }, [sdk, walletAddress, showTransactions, maxTransactions, refreshInterval]);

  // Format balance with WRF symbol
  const formatBalance = balance => {
    if (balance === undefined || balance === null) return 'N/A';
    return `${parseFloat(balance).toLocaleString(undefined, { minimumFractionDigits: 2, maximumFractionDigits: 6 })} WRF`;
  };

  // Format timestamp
  const formatTimestamp = timestamp => {
    if (!timestamp) return '';

    const date = new Date(timestamp);
    return date.toLocaleString();
  };

  // Handle transaction click
  const handleTransactionClick = transaction => {
    if (onTransactionClick && typeof onTransactionClick === 'function') {
      onTransactionClick(transaction);
    }
  };

  // Handle transfer click
  const handleTransferClick = () => {
    if (onTransferClick && typeof onTransferClick === 'function') {
      onTransferClick(walletAddress);
    }
  };

  // Format transaction amount (add + or - sign)
  const formatTransactionAmount = transaction => {
    if (transaction.amount === undefined || transaction.amount === null)
      return 'N/A';

    // Determine if this is an incoming or outgoing transaction
    const isOutgoing = transaction.from === walletAddress;
    const prefix = isOutgoing ? '-' : '+';
    const amount = parseFloat(transaction.amount);

    return `${prefix} ${amount.toLocaleString(undefined, { minimumFractionDigits: 2, maximumFractionDigits: 6 })} WRF`;
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

  // Get transaction amount color
  const getTransactionAmountColor = transaction => {
    const isOutgoing = transaction.from === walletAddress;

    if (isOutgoing) {
      return theme === 'dark' ? '#ff8a8a' : '#cc0000';
    } else {
      return theme === 'dark' ? '#8aff8a' : '#008000';
    }
  };

  // Format wallet address for display
  const formatWalletAddress = address => {
    if (!address) return '';

    // Return first 6 and last 4 characters
    return `${address.slice(0, 6)}...${address.slice(-4)}`;
  };

  // Generate CSS for the component
  const styles = {
    container: {
      backgroundColor: theme === 'dark' ? '#333' : '#fff',
      color: theme === 'dark' ? '#fff' : '#333',
      border: theme === 'dark' ? '1px solid #444' : '1px solid #ddd',
      borderRadius: '8px',
      padding: '16px',
      maxWidth: '100%',
    },
    header: {
      marginBottom: '16px',
    },
    title: {
      fontSize: '18px',
      fontWeight: '600',
      marginBottom: '4px',
    },
    address: {
      fontSize: '14px',
      color: theme === 'dark' ? '#aaa' : '#777',
      wordBreak: 'break-all',
    },
    balanceSection: {
      marginBottom: '16px',
      padding: '16px',
      backgroundColor: theme === 'dark' ? '#222' : '#f9f9f9',
      borderRadius: '4px',
    },
    balanceLabel: {
      fontSize: '14px',
      color: theme === 'dark' ? '#aaa' : '#777',
      marginBottom: '4px',
    },
    balanceValue: {
      fontSize: '24px',
      fontWeight: '600',
    },
    transactionsHeader: {
      display: 'flex',
      justifyContent: 'space-between',
      alignItems: 'center',
      marginBottom: '12px',
    },
    transactionsTitle: {
      fontSize: '16px',
      fontWeight: '600',
    },
    transactionList: {
      maxHeight: '300px',
      overflowY: 'auto',
    },
    transactionItem: {
      padding: '10px',
      borderBottom: theme === 'dark' ? '1px solid #444' : '1px solid #eee',
      display: 'flex',
      justifyContent: 'space-between',
      alignItems: 'center',
      cursor: onTransactionClick ? 'pointer' : 'default',
      transition: 'background-color 0.2s ease',
      '&:hover': {
        backgroundColor: theme === 'dark' ? '#3a3a3a' : '#f5f5f5',
      },
    },
    transactionInfo: {
      display: 'flex',
      flexDirection: 'column',
    },
    transactionType: {
      fontSize: '14px',
      fontWeight: '500',
    },
    transactionTime: {
      fontSize: '12px',
      color: theme === 'dark' ? '#aaa' : '#777',
    },
    transactionAmount: transaction => ({
      fontSize: '14px',
      fontWeight: '500',
      color: getTransactionAmountColor(transaction),
    }),
    transferButton: {
      padding: '8px 16px',
      backgroundColor: theme === 'dark' ? '#3a8eff' : '#3a8eff',
      color: '#fff',
      border: 'none',
      borderRadius: '4px',
      cursor: 'pointer',
      fontSize: '14px',
      fontWeight: '500',
      transition: 'background-color 0.2s ease',
      marginTop: '16px',
      '&:hover': {
        backgroundColor: theme === 'dark' ? '#2a7ae9' : '#2a7ae9',
      },
    },
    noTransactions: {
      padding: '16px',
      textAlign: 'center',
      color: theme === 'dark' ? '#aaa' : '#777',
      fontSize: '14px',
    },
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
  };

  return (
    <div style={styles.container} className="wallet-balance">
      {loading ? (
        <div style={styles.loading}>Loading wallet information...</div>
      ) : error ? (
        <div style={styles.error}>{error}</div>
      ) : walletInfo ? (
        <>
          <div style={styles.header}>
            <div style={styles.title}>Wallet</div>
            <div style={styles.address} title={walletAddress}>
              {formatWalletAddress(walletAddress)}
            </div>
          </div>

          <div style={styles.balanceSection}>
            <div style={styles.balanceLabel}>Current Balance</div>
            <div style={styles.balanceValue}>
              {formatBalance(walletInfo.balance)}
            </div>
          </div>

          {showTransactions && (
            <>
              <div style={styles.transactionsHeader}>
                <div style={styles.transactionsTitle}>Recent Transactions</div>
              </div>

              {transactions.length === 0 ? (
                <div style={styles.noTransactions}>No transactions found</div>
              ) : (
                <div style={styles.transactionList}>
                  {transactions.map(transaction => (
                    <div
                      key={transaction.id}
                      style={styles.transactionItem}
                      onClick={() => handleTransactionClick(transaction)}
                      className="transaction-item"
                    >
                      <div style={styles.transactionInfo}>
                        <div style={styles.transactionType}>
                          {getTransactionTypeDisplay(transaction.type)}
                        </div>
                        <div style={styles.transactionTime}>
                          {formatTimestamp(transaction.timestamp)}
                        </div>
                      </div>

                      <div style={styles.transactionAmount(transaction)}>
                        {formatTransactionAmount(transaction)}
                      </div>
                    </div>
                  ))}
                </div>
              )}
            </>
          )}

          {showTransfer && (
            <button
              style={styles.transferButton}
              onClick={handleTransferClick}
              className="transfer-button"
            >
              Transfer WRF
            </button>
          )}
        </>
      ) : (
        <div style={styles.error}>Wallet not found</div>
      )}
    </div>
  );
};

WalletBalance.propTypes = {
  walletAddress: PropTypes.string.isRequired,
  showTransactions: PropTypes.bool,
  maxTransactions: PropTypes.number,
  showTransfer: PropTypes.bool,
  theme: PropTypes.oneOf(['light', 'dark']),
  refreshInterval: PropTypes.number,
  onTransactionClick: PropTypes.func,
  onTransferClick: PropTypes.func,
};

export default WalletBalance;
