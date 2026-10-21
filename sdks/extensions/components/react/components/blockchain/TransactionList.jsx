import React, { useState, useEffect } from 'react';
import PropTypes from 'prop-types';
import { useWrfcoinContext } from '../../context/WrfcoinContext';

/**
 * TransactionList component for displaying blockchain transactions
 *
 * This component displays a list of transactions with filtering,
 * sorting, and pagination capabilities.
 */
const TransactionList = ({
  maxItems = 10,
  theme = 'light',
  transactionTypes = null,
  initialSort = 'timestamp',
  showDetails = true,
  enableFilters = true,
  walletAddress = null,
  refreshInterval = 30000,
  onTransactionSelect,
}) => {
  const { sdk } = useWrfcoinContext();
  const [transactions, setTransactions] = useState([]);
  const [filteredTransactions, setFilteredTransactions] = useState([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(null);
  const [sortOrder, setSortOrder] = useState(initialSort);
  const [sortDirection, setSortDirection] = useState('desc');
  const [currentPage, setCurrentPage] = useState(1);
  const [filter, setFilter] = useState('');
  const [selectedType, setSelectedType] = useState('all');
  const [selectedStatus, setSelectedStatus] = useState('all');
  const [selectedTransaction, setSelectedTransaction] = useState(null);
  const [networkInfo, setNetworkInfo] = useState({});

  // Fetch transactions
  useEffect(() => {
    if (!sdk) return;

    const fetchTransactions = async () => {
      try {
        setLoading(true);
        setError(null);

        // Set up query parameters
        const params = {};

        if (walletAddress) {
          params.address = walletAddress;
        }

        if (
          transactionTypes &&
          Array.isArray(transactionTypes) &&
          transactionTypes.length > 0
        ) {
          params.types = transactionTypes;
        }

        // Fetch transactions
        const result = await sdk.getTransactions(params);

        if (result && result.transactions) {
          setTransactions(result.transactions);
          setFilteredTransactions(result.transactions);
        } else {
          setTransactions([]);
          setFilteredTransactions([]);
        }

        // Set network info if available
        if (result && result.networkInfo) {
          setNetworkInfo(result.networkInfo);
        }
      } catch (err) {
        console.error('Error fetching transactions:', err);
        setError('Failed to load transactions. Please try again later.');
      } finally {
        setLoading(false);
      }
    };

    // Initial fetch
    fetchTransactions();

    // Set up interval for refreshing data
    const intervalId = setInterval(fetchTransactions, refreshInterval);

    return () => {
      clearInterval(intervalId);
    };
  }, [sdk, walletAddress, transactionTypes, refreshInterval]);

  // Filter and sort transactions
  useEffect(() => {
    let filtered = [...transactions];

    // Filter by search text
    if (filter) {
      const lowerFilter = filter.toLowerCase();
      filtered = filtered.filter(
        tx =>
          tx.id.toLowerCase().includes(lowerFilter) ||
          (tx.from && tx.from.toLowerCase().includes(lowerFilter)) ||
          (tx.to && tx.to.toLowerCase().includes(lowerFilter)) ||
          (tx.data && tx.data.toLowerCase().includes(lowerFilter))
      );
    }

    // Filter by transaction type
    if (selectedType !== 'all') {
      filtered = filtered.filter(tx => tx.type === selectedType);
    }

    // Filter by status
    if (selectedStatus !== 'all') {
      filtered = filtered.filter(tx => tx.status === selectedStatus);
    }

    // Sort transactions
    filtered.sort((a, b) => {
      let comparison = 0;

      switch (sortOrder) {
        case 'timestamp':
          comparison = new Date(a.timestamp) - new Date(b.timestamp);
          break;
        case 'amount':
          comparison = parseFloat(a.amount) - parseFloat(b.amount);
          break;
        case 'fee':
          comparison = parseFloat(a.fee) - parseFloat(b.fee);
          break;
        case 'id':
          comparison = a.id.localeCompare(b.id);
          break;
        default:
          comparison = new Date(a.timestamp) - new Date(b.timestamp);
      }

      return sortDirection === 'asc' ? comparison : -comparison;
    });

    setFilteredTransactions(filtered);
    setCurrentPage(1);
  }, [
    transactions,
    filter,
    selectedType,
    selectedStatus,
    sortOrder,
    sortDirection,
  ]);

  // Get unique transaction types
  const getTransactionTypes = () => {
    const types = new Set();
    transactions.forEach(tx => {
      if (tx.type) types.add(tx.type);
    });
    return ['all', ...Array.from(types)];
  };

  // Get unique transaction statuses
  const getTransactionStatuses = () => {
    const statuses = new Set();
    transactions.forEach(tx => {
      if (tx.status) statuses.add(tx.status);
    });
    return ['all', ...Array.from(statuses)];
  };

  // Handle transaction click
  const handleTransactionClick = tx => {
    setSelectedTransaction(tx.id === selectedTransaction ? null : tx.id);

    if (onTransactionSelect && typeof onTransactionSelect === 'function') {
      onTransactionSelect(tx);
    }
  };

  // Get items for current page
  const getCurrentPageItems = () => {
    const startIndex = (currentPage - 1) * maxItems;
    const endIndex = startIndex + maxItems;

    return filteredTransactions.slice(startIndex, endIndex);
  };

  // Handle filter change
  const handleFilterChange = e => {
    setFilter(e.target.value);
  };

  // Handle type filter change
  const handleTypeChange = e => {
    setSelectedType(e.target.value);
  };

  // Handle status filter change
  const handleStatusChange = e => {
    setSelectedStatus(e.target.value);
  };

  // Handle sort change
  const handleSortChange = e => {
    setSortOrder(e.target.value);
  };

  // Handle sort direction change
  const handleSortDirectionToggle = () => {
    setSortDirection(sortDirection === 'asc' ? 'desc' : 'asc');
  };

  // Format timestamp
  const formatTimestamp = timestamp => {
    if (!timestamp) return 'Unknown';

    const date = new Date(timestamp);
    return date.toLocaleString();
  };

  // Format amount
  const formatAmount = amount => {
    if (amount === undefined || amount === null) return '0';

    // Format large numbers
    if (amount >= 1000000) {
      return `${(amount / 1000000).toFixed(2)}M`;
    } else if (amount >= 1000) {
      return `${(amount / 1000).toFixed(2)}K`;
    }

    return amount.toString();
  };

  // Get color for transaction type
  const getTransactionTypeColor = type => {
    switch (type) {
      case 'Regular':
        return theme === 'dark' ? '#90caf9' : '#2196f3';
      case 'Computational':
        return theme === 'dark' ? '#ce93d8' : '#9c27b0';
      case 'Governance':
        return theme === 'dark' ? '#80cbc4' : '#009688';
      case 'NFT':
        return theme === 'dark' ? '#ffcc80' : '#ff9800';
      default:
        return theme === 'dark' ? '#e0e0e0' : '#9e9e9e';
    }
  };

  // Get color for transaction status
  const getStatusColor = status => {
    switch (status) {
      case 'confirmed':
        return theme === 'dark' ? '#a5d6a7' : '#4caf50';
      case 'pending':
        return theme === 'dark' ? '#fff59d' : '#ffeb3b';
      case 'failed':
        return theme === 'dark' ? '#ef9a9a' : '#f44336';
      default:
        return theme === 'dark' ? '#e0e0e0' : '#9e9e9e';
    }
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
      alignItems: 'flex-start',
    },
    titleContainer: {
      display: 'flex',
      flexDirection: 'column',
    },
    title: {
      fontSize: '18px',
      fontWeight: '600',
      marginBottom: '4px',
    },
    subtitle: {
      fontSize: '14px',
      color: theme === 'dark' ? '#aaa' : '#777',
    },
    networkInfo: {
      display: 'flex',
      flexDirection: 'column',
      alignItems: 'flex-end',
      gap: '4px',
    },
    networkInfoItem: {
      fontSize: '13px',
      color: theme === 'dark' ? '#ddd' : '#555',
    },
    controls: {
      display: 'flex',
      flexWrap: 'wrap',
      gap: '8px',
      marginBottom: '16px',
    },
    filterGroup: {
      flex: '1',
      minWidth: '200px',
      display: 'flex',
      flexDirection: 'column',
      gap: '8px',
    },
    filterLabel: {
      fontSize: '14px',
      fontWeight: '500',
    },
    filter: {
      padding: '8px 12px',
      borderRadius: '4px',
      border: theme === 'dark' ? '1px solid #555' : '1px solid #ddd',
      backgroundColor: theme === 'dark' ? '#444' : '#fff',
      color: theme === 'dark' ? '#fff' : '#333',
      fontSize: '14px',
      width: '100%',
    },
    select: {
      padding: '8px 12px',
      borderRadius: '4px',
      border: theme === 'dark' ? '1px solid #555' : '1px solid #ddd',
      backgroundColor: theme === 'dark' ? '#444' : '#fff',
      color: theme === 'dark' ? '#fff' : '#333',
      fontSize: '14px',
    },
    sortContainer: {
      display: 'flex',
      alignItems: 'center',
      gap: '8px',
    },
    sortButton: {
      padding: '8px 12px',
      backgroundColor: theme === 'dark' ? '#444' : '#f5f5f5',
      color: theme === 'dark' ? '#fff' : '#333',
      border: theme === 'dark' ? '1px solid #555' : '1px solid #ddd',
      borderRadius: '4px',
      cursor: 'pointer',
      fontSize: '14px',
      display: 'flex',
      alignItems: 'center',
      gap: '4px',
    },
    table: {
      width: '100%',
      borderCollapse: 'collapse',
      fontSize: '14px',
    },
    tableHead: {
      backgroundColor: theme === 'dark' ? '#222' : '#f5f5f5',
      color: theme === 'dark' ? '#fff' : '#333',
      textAlign: 'left',
    },
    tableHeader: {
      padding: '12px 8px',
      fontWeight: '600',
      borderBottom: theme === 'dark' ? '1px solid #444' : '1px solid #ddd',
      cursor: 'pointer',
      userSelect: 'none',
    },
    tableRow: isSelected => ({
      backgroundColor: isSelected
        ? theme === 'dark'
          ? '#1c4373'
          : '#e3f2fd'
        : 'transparent',
      cursor: 'pointer',
      '&:hover': {
        backgroundColor: theme === 'dark' ? '#444' : '#f9f9f9',
      },
    }),
    tableCell: {
      padding: '12px 8px',
      borderBottom: theme === 'dark' ? '1px solid #444' : '1px solid #ddd',
    },
    typeBadge: type => ({
      display: 'inline-block',
      padding: '4px 8px',
      borderRadius: '4px',
      backgroundColor: getTransactionTypeColor(type),
      color: theme === 'dark' ? '#000' : '#fff',
      fontSize: '12px',
      fontWeight: '500',
    }),
    statusBadge: status => ({
      display: 'inline-block',
      padding: '4px 8px',
      borderRadius: '4px',
      backgroundColor: getStatusColor(status),
      color: theme === 'dark' ? '#000' : '#fff',
      fontSize: '12px',
      fontWeight: '500',
      marginLeft: '4px',
    }),
    transactionDetails: {
      backgroundColor: theme === 'dark' ? '#222' : '#f9f9f9',
      padding: '16px',
      marginTop: '8px',
      marginBottom: '8px',
      borderRadius: '4px',
      fontSize: '14px',
    },
    detailItem: {
      display: 'flex',
      justifyContent: 'space-between',
      marginBottom: '8px',
    },
    detailLabel: {
      fontWeight: '500',
      marginRight: '16px',
    },
    detailValue: {
      wordBreak: 'break-all',
    },
    pagination: {
      display: 'flex',
      justifyContent: 'space-between',
      alignItems: 'center',
      marginTop: '16px',
    },
    pageInfo: {
      fontSize: '14px',
      color: theme === 'dark' ? '#aaa' : '#777',
    },
    pageButtons: {
      display: 'flex',
      gap: '8px',
    },
    pageButton: isActive => ({
      padding: '6px 12px',
      backgroundColor: isActive
        ? theme === 'dark'
          ? '#3a8eff'
          : '#3a8eff'
        : theme === 'dark'
          ? '#444'
          : '#f5f5f5',
      color: isActive ? '#fff' : theme === 'dark' ? '#fff' : '#333',
      border: 'none',
      borderRadius: '4px',
      cursor: isActive ? 'default' : 'pointer',
      fontSize: '14px',
      opacity: isActive ? 1 : 0.8,
    }),
    noTransactions: {
      textAlign: 'center',
      padding: '32px 16px',
      color: theme === 'dark' ? '#aaa' : '#777',
      backgroundColor: theme === 'dark' ? '#222' : '#f9f9f9',
      borderRadius: '4px',
    },
    loading: {
      textAlign: 'center',
      padding: '32px 16px',
      color: theme === 'dark' ? '#aaa' : '#777',
      backgroundColor: theme === 'dark' ? '#222' : '#f9f9f9',
      borderRadius: '4px',
    },
    error: {
      textAlign: 'center',
      padding: '32px 16px',
      color: theme === 'dark' ? '#ff8a8a' : '#cc0000',
      backgroundColor: theme === 'dark' ? '#222' : '#f9f9f9',
      borderRadius: '4px',
    },
    addressText: {
      maxWidth: '150px',
      overflow: 'hidden',
      textOverflow: 'ellipsis',
      whiteSpace: 'nowrap',
      display: 'inline-block',
    },
  };

  // Render transaction row
  const renderTransactionRow = tx => {
    const isSelected = selectedTransaction === tx.id;

    return (
      <React.Fragment key={tx.id}>
        <tr
          onClick={() => handleTransactionClick(tx)}
          style={styles.tableRow(isSelected)}
        >
          <td style={styles.tableCell}>
            <div style={{ fontFamily: 'monospace' }}>{tx.id}</div>
          </td>
          <td style={styles.tableCell}>
            <div style={styles.typeBadge(tx.type)}>{tx.type}</div>
            {tx.status && (
              <div style={styles.statusBadge(tx.status)}>{tx.status}</div>
            )}
          </td>
          <td style={styles.tableCell}>{formatTimestamp(tx.timestamp)}</td>
          <td style={styles.tableCell}>
            {tx.from && (
              <div style={styles.addressText} title={tx.from}>
                {tx.from}
              </div>
            )}
          </td>
          <td style={styles.tableCell}>
            {tx.to && (
              <div style={styles.addressText} title={tx.to}>
                {tx.to}
              </div>
            )}
          </td>
          <td style={styles.tableCell}>{formatAmount(tx.amount)}</td>
          <td style={styles.tableCell}>{tx.fee}</td>
        </tr>

        {showDetails && isSelected && (
          <tr>
            <td colSpan="7" style={{ padding: 0 }}>
              <div style={styles.transactionDetails}>
                <div style={styles.detailItem}>
                  <div style={styles.detailLabel}>Transaction ID:</div>
                  <div style={styles.detailValue}>{tx.id}</div>
                </div>
                <div style={styles.detailItem}>
                  <div style={styles.detailLabel}>Timestamp:</div>
                  <div style={styles.detailValue}>
                    {formatTimestamp(tx.timestamp)}
                  </div>
                </div>
                <div style={styles.detailItem}>
                  <div style={styles.detailLabel}>Type:</div>
                  <div style={styles.detailValue}>{tx.type}</div>
                </div>
                <div style={styles.detailItem}>
                  <div style={styles.detailLabel}>Status:</div>
                  <div style={styles.detailValue}>{tx.status}</div>
                </div>
                {tx.from && (
                  <div style={styles.detailItem}>
                    <div style={styles.detailLabel}>From:</div>
                    <div style={styles.detailValue}>{tx.from}</div>
                  </div>
                )}
                {tx.to && (
                  <div style={styles.detailItem}>
                    <div style={styles.detailLabel}>To:</div>
                    <div style={styles.detailValue}>{tx.to}</div>
                  </div>
                )}
                <div style={styles.detailItem}>
                  <div style={styles.detailLabel}>Amount:</div>
                  <div style={styles.detailValue}>{tx.amount} WRF</div>
                </div>
                <div style={styles.detailItem}>
                  <div style={styles.detailLabel}>Fee:</div>
                  <div style={styles.detailValue}>{tx.fee} WRF</div>
                </div>
                {tx.blockNumber && (
                  <div style={styles.detailItem}>
                    <div style={styles.detailLabel}>Block Number:</div>
                    <div style={styles.detailValue}>{tx.blockNumber}</div>
                  </div>
                )}
                {tx.blockHash && (
                  <div style={styles.detailItem}>
                    <div style={styles.detailLabel}>Block Hash:</div>
                    <div style={styles.detailValue}>{tx.blockHash}</div>
                  </div>
                )}
                {tx.data && (
                  <div style={styles.detailItem}>
                    <div style={styles.detailLabel}>Data:</div>
                    <div style={styles.detailValue}>{tx.data}</div>
                  </div>
                )}
              </div>
            </td>
          </tr>
        )}
      </React.Fragment>
    );
  };

  // Render pagination
  const renderPagination = () => {
    const totalPages = Math.ceil(filteredTransactions.length / maxItems);

    if (totalPages <= 1) return null;

    return (
      <div style={styles.pagination}>
        <div style={styles.pageInfo}>
          Showing {(currentPage - 1) * maxItems + 1} to{' '}
          {Math.min(currentPage * maxItems, filteredTransactions.length)} of{' '}
          {filteredTransactions.length} transactions
        </div>

        <div style={styles.pageButtons}>
          <button
            style={styles.pageButton(false)}
            onClick={() => setCurrentPage(1)}
            disabled={currentPage === 1}
          >
            First
          </button>

          <button
            style={styles.pageButton(false)}
            onClick={() => setCurrentPage(currentPage - 1)}
            disabled={currentPage === 1}
          >
            Previous
          </button>

          {Array.from({ length: Math.min(5, totalPages) }, (_, i) => {
            let pageNum;
            if (totalPages <= 5) {
              pageNum = i + 1;
            } else if (currentPage <= 3) {
              pageNum = i + 1;
            } else if (currentPage >= totalPages - 2) {
              pageNum = totalPages - 4 + i;
            } else {
              pageNum = currentPage - 2 + i;
            }

            return (
              <button
                key={pageNum}
                style={styles.pageButton(currentPage === pageNum)}
                onClick={() => setCurrentPage(pageNum)}
              >
                {pageNum}
              </button>
            );
          })}

          <button
            style={styles.pageButton(false)}
            onClick={() => setCurrentPage(currentPage + 1)}
            disabled={currentPage === totalPages}
          >
            Next
          </button>

          <button
            style={styles.pageButton(false)}
            onClick={() => setCurrentPage(totalPages)}
            disabled={currentPage === totalPages}
          >
            Last
          </button>
        </div>
      </div>
    );
  };

  return (
    <div style={styles.container} className="transaction-list">
      <div style={styles.header}>
        <div style={styles.titleContainer}>
          <div style={styles.title}>Transaction List</div>
          <div style={styles.subtitle}>
            {walletAddress ? `Wallet: ${walletAddress}` : 'All transactions'}
          </div>
        </div>

        {networkInfo && Object.keys(networkInfo).length > 0 && (
          <div style={styles.networkInfo}>
            <div style={styles.networkInfoItem}>
              Network: {networkInfo.network || 'Unknown'}
            </div>
            {networkInfo.blockHeight && (
              <div style={styles.networkInfoItem}>
                Block Height: {networkInfo.blockHeight}
              </div>
            )}
          </div>
        )}
      </div>

      {enableFilters && (
        <div style={styles.controls}>
          <div style={styles.filterGroup}>
            <div style={styles.filterLabel}>Search Transactions</div>
            <input
              type="text"
              style={styles.filter}
              placeholder="Search by ID, address, or data..."
              value={filter}
              onChange={handleFilterChange}
            />
          </div>

          <div style={styles.filterGroup}>
            <div style={styles.filterLabel}>Filter by Type</div>
            <select
              style={styles.select}
              value={selectedType}
              onChange={handleTypeChange}
            >
              {getTransactionTypes().map(type => (
                <option key={type} value={type}>
                  {type === 'all' ? 'All Types' : type}
                </option>
              ))}
            </select>
          </div>

          <div style={styles.filterGroup}>
            <div style={styles.filterLabel}>Filter by Status</div>
            <select
              style={styles.select}
              value={selectedStatus}
              onChange={handleStatusChange}
            >
              {getTransactionStatuses().map(status => (
                <option key={status} value={status}>
                  {status === 'all' ? 'All Statuses' : status}
                </option>
              ))}
            </select>
          </div>

          <div style={styles.filterGroup}>
            <div style={styles.filterLabel}>Sort By</div>
            <div style={styles.sortContainer}>
              <select
                style={styles.select}
                value={sortOrder}
                onChange={handleSortChange}
              >
                <option value="timestamp">Timestamp</option>
                <option value="amount">Amount</option>
                <option value="fee">Fee</option>
                <option value="id">Transaction ID</option>
              </select>

              <button
                style={styles.sortButton}
                onClick={handleSortDirectionToggle}
              >
                {sortDirection === 'asc' ? '↑ Asc' : '↓ Desc'}
              </button>
            </div>
          </div>
        </div>
      )}

      {loading ? (
        <div style={styles.loading}>Loading transactions...</div>
      ) : error ? (
        <div style={styles.error}>{error}</div>
      ) : filteredTransactions.length === 0 ? (
        <div style={styles.noTransactions}>No transactions found</div>
      ) : (
        <>
          <div style={{ overflowX: 'auto' }}>
            <table style={styles.table}>
              <thead style={styles.tableHead}>
                <tr>
                  <th style={styles.tableHeader}>Transaction ID</th>
                  <th style={styles.tableHeader}>Type</th>
                  <th style={styles.tableHeader}>Timestamp</th>
                  <th style={styles.tableHeader}>From</th>
                  <th style={styles.tableHeader}>To</th>
                  <th style={styles.tableHeader}>Amount</th>
                  <th style={styles.tableHeader}>Fee</th>
                </tr>
              </thead>
              <tbody>{getCurrentPageItems().map(renderTransactionRow)}</tbody>
            </table>
          </div>

          {renderPagination()}
        </>
      )}
    </div>
  );
};

TransactionList.propTypes = {
  maxItems: PropTypes.number,
  theme: PropTypes.oneOf(['light', 'dark']),
  transactionTypes: PropTypes.arrayOf(PropTypes.string),
  initialSort: PropTypes.oneOf(['timestamp', 'amount', 'fee', 'id']),
  showDetails: PropTypes.bool,
  enableFilters: PropTypes.bool,
  walletAddress: PropTypes.string,
  refreshInterval: PropTypes.number,
  onTransactionSelect: PropTypes.func,
};

export default TransactionList;
