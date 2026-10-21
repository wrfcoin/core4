import React, { useState, useEffect } from 'react';
import PropTypes from 'prop-types';
import { useWrfcoinContext } from '../../context/WrfcoinContext';
import {
  LineChart,
  Line,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  Legend,
  ResponsiveContainer,
} from 'recharts';

/**
 * WalletHistory component for displaying detailed wallet transaction history
 *
 * This component shows a detailed transaction history with filtering options
 * and a chart visualization of balance over time.
 */
const WalletHistory = ({
  walletAddress,
  days = 30,
  showChart = true,
  showTypeFilter = true,
  maxTransactions = 50,
  theme = 'light',
  onTransactionClick,
}) => {
  const { sdk } = useWrfcoinContext();
  const [transactions, setTransactions] = useState([]);
  const [balanceHistory, setBalanceHistory] = useState([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(null);
  const [filteredTransactions, setFilteredTransactions] = useState([]);
  const [typeFilter, setTypeFilter] = useState('all');

  // Transaction types
  const transactionTypes = [
    'all',
    'Regular',
    'NFT',
    'Computational',
    'Governance',
    'WeatherData',
    'ForecastData',
    'StationRegistration',
    'Validator',
  ];

  // Fetch transactions and balance history
  useEffect(() => {
    if (!sdk || !walletAddress) return;

    const fetchWalletHistory = async () => {
      try {
        setLoading(true);
        setError(null);

        // Fetch transactions
        const txData = await sdk.getTransactions({
          wallet: walletAddress,
          limit: maxTransactions,
          days: days,
        });

        setTransactions(txData || []);
        setFilteredTransactions(txData || []);

        // Fetch balance history
        const historyData = await sdk.getBalanceHistory({
          wallet: walletAddress,
          days: days,
        });

        setBalanceHistory(historyData || []);
      } catch (err) {
        console.error('Error fetching wallet history:', err);
        setError('Failed to load wallet history. Please try again later.');
      } finally {
        setLoading(false);
      }
    };

    fetchWalletHistory();
  }, [sdk, walletAddress, days, maxTransactions]);

  // Filter transactions when type filter changes
  useEffect(() => {
    if (typeFilter === 'all') {
      setFilteredTransactions(transactions);
    } else {
      setFilteredTransactions(
        transactions.filter(tx => tx.type === typeFilter)
      );
    }
  }, [transactions, typeFilter]);

  // Format timestamp
  const formatTimestamp = timestamp => {
    if (!timestamp) return '';

    const date = new Date(timestamp);
    return date.toLocaleString();
  };

  // Format chart date
  const formatChartDate = timestamp => {
    if (!timestamp) return '';

    const date = new Date(timestamp);
    return `${date.getMonth() + 1}/${date.getDate()}`;
  };

  // Handle transaction click
  const handleTransactionClick = transaction => {
    if (onTransactionClick && typeof onTransactionClick === 'function') {
      onTransactionClick(transaction);
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

  // Format balance for chart
  const formatBalance = balance => {
    if (balance === undefined || balance === null) return 'N/A';
    return parseFloat(balance).toLocaleString(undefined, {
      minimumFractionDigits: 2,
      maximumFractionDigits: 6,
    });
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
      display: 'flex',
      justifyContent: 'space-between',
      alignItems: 'center',
      flexWrap: 'wrap',
    },
    title: {
      fontSize: '18px',
      fontWeight: '600',
    },
    filterContainer: {
      display: 'flex',
      alignItems: 'center',
      marginTop: '8px',
    },
    filterLabel: {
      marginRight: '8px',
      fontSize: '14px',
    },
    filterSelect: {
      padding: '4px 8px',
      borderRadius: '4px',
      backgroundColor: theme === 'dark' ? '#444' : '#f5f5f5',
      color: theme === 'dark' ? '#fff' : '#333',
      border: theme === 'dark' ? '1px solid #555' : '1px solid #ddd',
    },
    chartContainer: {
      height: '200px',
      marginBottom: '16px',
    },
    transactionList: {
      maxHeight: '400px',
      overflowY: 'auto',
    },
    transactionItem: {
      padding: '12px',
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
      flex: '1',
    },
    transactionType: {
      fontSize: '14px',
      fontWeight: '500',
      display: 'flex',
      alignItems: 'center',
    },
    transactionTypeTag: type => ({
      padding: '2px 6px',
      borderRadius: '4px',
      fontSize: '12px',
      marginRight: '8px',
      backgroundColor: getTypeColor(type, theme),
      color: '#fff',
    }),
    transactionDetails: {
      display: 'flex',
      alignItems: 'center',
      marginTop: '4px',
    },
    transactionTime: {
      fontSize: '12px',
      color: theme === 'dark' ? '#aaa' : '#777',
    },
    transactionId: {
      fontSize: '12px',
      color: theme === 'dark' ? '#aaa' : '#777',
      marginLeft: '8px',
    },
    transactionAmount: transaction => ({
      fontSize: '14px',
      fontWeight: '500',
      color: getTransactionAmountColor(transaction),
      marginLeft: '8px',
      whiteSpace: 'nowrap',
    }),
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

  // Get color for transaction type tag
  function getTypeColor(type, theme) {
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
        return theme === 'dark' ? '#555' : '#999';
    }
  }

  return (
    <div style={styles.container} className="wallet-history">
      {loading ? (
        <div style={styles.loading}>Loading wallet history...</div>
      ) : error ? (
        <div style={styles.error}>{error}</div>
      ) : (
        <>
          <div style={styles.header}>
            <div style={styles.title}>Transaction History</div>

            {showTypeFilter && (
              <div style={styles.filterContainer}>
                <div style={styles.filterLabel}>Filter:</div>
                <select
                  style={styles.filterSelect}
                  value={typeFilter}
                  onChange={e => setTypeFilter(e.target.value)}
                >
                  {transactionTypes.map(type => (
                    <option key={type} value={type}>
                      {type === 'all'
                        ? 'All Types'
                        : getTransactionTypeDisplay(type)}
                    </option>
                  ))}
                </select>
              </div>
            )}
          </div>

          {showChart && balanceHistory.length > 0 && (
            <div style={styles.chartContainer}>
              <ResponsiveContainer width="100%" height="100%">
                <LineChart
                  data={balanceHistory}
                  margin={{ top: 5, right: 5, left: 5, bottom: 5 }}
                >
                  <CartesianGrid
                    strokeDasharray="3 3"
                    stroke={theme === 'dark' ? '#444' : '#eee'}
                  />
                  <XAxis
                    dataKey="timestamp"
                    tick={{ fill: theme === 'dark' ? '#ddd' : '#666' }}
                    tickFormatter={formatChartDate}
                  />
                  <YAxis tick={{ fill: theme === 'dark' ? '#ddd' : '#666' }} />
                  <Tooltip
                    labelFormatter={timestamp =>
                      `Date: ${new Date(timestamp).toLocaleDateString()}`
                    }
                    formatter={value => [
                      `${formatBalance(value)} WRF`,
                      'Balance',
                    ]}
                    contentStyle={{
                      backgroundColor: theme === 'dark' ? '#333' : '#fff',
                      border:
                        theme === 'dark' ? '1px solid #444' : '1px solid #ddd',
                      color: theme === 'dark' ? '#fff' : '#333',
                    }}
                  />
                  <Legend />
                  <Line
                    type="monotone"
                    dataKey="balance"
                    name="Balance"
                    stroke="#3a8eff"
                    activeDot={{ r: 8 }}
                    dot={{ r: 4 }}
                  />
                </LineChart>
              </ResponsiveContainer>
            </div>
          )}

          {filteredTransactions.length === 0 ? (
            <div style={styles.noTransactions}>No transactions found</div>
          ) : (
            <div style={styles.transactionList}>
              {filteredTransactions.map(transaction => (
                <div
                  key={transaction.id}
                  style={styles.transactionItem}
                  onClick={() => handleTransactionClick(transaction)}
                  className="transaction-item"
                >
                  <div style={styles.transactionInfo}>
                    <div style={styles.transactionType}>
                      <span style={styles.transactionTypeTag(transaction.type)}>
                        {getTransactionTypeDisplay(transaction.type)}
                      </span>
                      {transaction.description && transaction.description}
                    </div>

                    <div style={styles.transactionDetails}>
                      <div style={styles.transactionTime}>
                        {formatTimestamp(transaction.timestamp)}
                      </div>
                      <div style={styles.transactionId}>
                        ID: {transaction.id}
                      </div>
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
    </div>
  );
};

WalletHistory.propTypes = {
  walletAddress: PropTypes.string.isRequired,
  days: PropTypes.number,
  showChart: PropTypes.bool,
  showTypeFilter: PropTypes.bool,
  maxTransactions: PropTypes.number,
  theme: PropTypes.oneOf(['light', 'dark']),
  onTransactionClick: PropTypes.func,
};

export default WalletHistory;
