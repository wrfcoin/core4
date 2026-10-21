import React, { useState, useEffect } from 'react';
import PropTypes from 'prop-types';
import { useWrfcoinContext } from '../../context/WrfcoinContext';

/**
 * WalletTransfer component for transferring WRF tokens to other wallets
 *
 * This component provides a simple form to transfer tokens with validation
 * and transaction status tracking.
 */
const WalletTransfer = ({
  walletAddress,
  onSuccess,
  onCancel,
  theme = 'light',
  maxAmount = null,
  validateAddress = true,
}) => {
  const { sdk } = useWrfcoinContext();
  const [recipient, setRecipient] = useState('');
  const [amount, setAmount] = useState('');
  const [memo, setMemo] = useState('');
  const [fee, setFee] = useState(null);
  const [walletInfo, setWalletInfo] = useState(null);
  const [loading, setLoading] = useState(false);
  const [estimatingFee, setEstimatingFee] = useState(false);
  const [error, setError] = useState(null);
  const [addressError, setAddressError] = useState(null);
  const [amountError, setAmountError] = useState(null);
  const [transactionStatus, setTransactionStatus] = useState(null);
  const [transactionHash, setTransactionHash] = useState(null);

  // Fetch wallet info
  useEffect(() => {
    if (!sdk || !walletAddress) return;

    const fetchWalletInfo = async () => {
      try {
        const info = await sdk.getWalletInfo(walletAddress);
        setWalletInfo(info);
      } catch (err) {
        console.error('Error fetching wallet info:', err);
        setError('Failed to load wallet information. Please try again later.');
      }
    };

    fetchWalletInfo();
  }, [sdk, walletAddress]);

  // Estimate fee when amount or recipient changes
  useEffect(() => {
    if (!sdk || !amount || amount <= 0 || !recipient) return;

    const estimateFee = async () => {
      try {
        setEstimatingFee(true);
        const feeEstimate = await sdk.estimateTransactionFee({
          from: walletAddress,
          to: recipient,
          amount: parseFloat(amount),
          type: 'Regular',
        });

        setFee(feeEstimate);
      } catch (err) {
        console.error('Error estimating fee:', err);
        setFee(null);
      } finally {
        setEstimatingFee(false);
      }
    };

    // Debounce fee estimation to avoid too many requests
    const timer = setTimeout(() => {
      estimateFee();
    }, 500);

    return () => clearTimeout(timer);
  }, [sdk, walletAddress, recipient, amount]);

  // Validate address format
  const validateAddressFormat = address => {
    if (!address) {
      setAddressError('Recipient address is required');
      return false;
    }

    // Basic validation (should be replaced with actual address validation logic)
    if (
      validateAddress &&
      (!address.startsWith('WRF') || address.length < 15)
    ) {
      setAddressError('Invalid WRF address format');
      return false;
    }

    // Prevent sending to self
    if (address === walletAddress) {
      setAddressError('Cannot send to your own address');
      return false;
    }

    setAddressError(null);
    return true;
  };

  // Validate amount
  const validateAmount = amountValue => {
    if (!amountValue || amountValue <= 0) {
      setAmountError('Amount must be greater than 0');
      return false;
    }

    // Convert to number
    const numAmount = parseFloat(amountValue);

    // Check if amount is a valid number
    if (isNaN(numAmount)) {
      setAmountError('Invalid amount');
      return false;
    }

    // Check if amount is greater than balance
    if (walletInfo && numAmount > walletInfo.balance) {
      setAmountError('Amount exceeds available balance');
      return false;
    }

    // Check if amount is greater than maximum allowed
    if (maxAmount !== null && numAmount > maxAmount) {
      setAmountError(`Amount exceeds maximum allowed (${maxAmount} WRF)`);
      return false;
    }

    // Check if the amount is too small
    if (numAmount < 0.000001) {
      setAmountError('Amount is too small (minimum 0.000001 WRF)');
      return false;
    }

    setAmountError(null);
    return true;
  };

  // Handle form submission
  const handleSubmit = async e => {
    e.preventDefault();

    // Reset errors
    setError(null);

    // Validate inputs
    const isAddressValid = validateAddressFormat(recipient);
    const isAmountValid = validateAmount(amount);

    if (!isAddressValid || !isAmountValid) {
      return;
    }

    // Start transaction
    try {
      setLoading(true);
      setTransactionStatus('pending');

      const transaction = await sdk.sendTransaction({
        from: walletAddress,
        to: recipient,
        amount: parseFloat(amount),
        memo: memo,
        type: 'Regular',
      });

      setTransactionHash(transaction.hash || transaction.id);
      setTransactionStatus('confirmed');

      // Call success callback
      if (onSuccess && typeof onSuccess === 'function') {
        onSuccess(transaction);
      }
    } catch (err) {
      console.error('Error sending transaction:', err);
      setError('Failed to send transaction. Please try again later.');
      setTransactionStatus('failed');
    } finally {
      setLoading(false);
    }
  };

  // Handle cancel button click
  const handleCancel = () => {
    if (onCancel && typeof onCancel === 'function') {
      onCancel();
    }
  };

  // Format fee
  const formatFee = feeValue => {
    if (!feeValue) return 'Calculating...';
    return `${parseFloat(feeValue).toFixed(6)} WRF`;
  };

  // Get total amount (including fee)
  const getTotalAmount = () => {
    if (!amount || !fee) return null;

    const totalAmount = parseFloat(amount) + parseFloat(fee);
    return totalAmount.toFixed(6);
  };

  // Get maximum sendable amount (balance - fee)
  const getMaxSendableAmount = () => {
    if (!walletInfo || !fee) return null;

    const maxSendable = Math.max(0, walletInfo.balance - parseFloat(fee));
    return maxSendable.toFixed(6);
  };

  // Set maximum amount
  const handleSetMaxAmount = () => {
    const maxSendable = getMaxSendableAmount();
    if (maxSendable) {
      setAmount(maxSendable);
      validateAmount(maxSendable);
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
    },
    title: {
      fontSize: '18px',
      fontWeight: '600',
      marginBottom: '4px',
    },
    form: {
      display: 'flex',
      flexDirection: 'column',
      gap: '16px',
    },
    formGroup: {
      display: 'flex',
      flexDirection: 'column',
      gap: '4px',
    },
    label: {
      fontSize: '14px',
      fontWeight: '500',
    },
    input: {
      padding: '8px 12px',
      borderRadius: '4px',
      border: theme === 'dark' ? '1px solid #555' : '1px solid #ddd',
      backgroundColor: theme === 'dark' ? '#444' : '#fff',
      color: theme === 'dark' ? '#fff' : '#333',
      fontSize: '14px',
    },
    textarea: {
      padding: '8px 12px',
      borderRadius: '4px',
      border: theme === 'dark' ? '1px solid #555' : '1px solid #ddd',
      backgroundColor: theme === 'dark' ? '#444' : '#fff',
      color: theme === 'dark' ? '#fff' : '#333',
      fontSize: '14px',
      minHeight: '60px',
      resize: 'vertical',
    },
    error: {
      color: theme === 'dark' ? '#ff8a8a' : '#cc0000',
      fontSize: '12px',
      marginTop: '4px',
    },
    feeContainer: {
      backgroundColor: theme === 'dark' ? '#222' : '#f9f9f9',
      padding: '12px',
      borderRadius: '4px',
      marginTop: '8px',
    },
    feeInfo: {
      display: 'flex',
      justifyContent: 'space-between',
      fontSize: '14px',
      marginBottom: '4px',
    },
    feeValue: {
      fontWeight: '500',
    },
    totalAmount: {
      display: 'flex',
      justifyContent: 'space-between',
      fontSize: '14px',
      fontWeight: '600',
      marginTop: '4px',
      paddingTop: '4px',
      borderTop: theme === 'dark' ? '1px solid #444' : '1px solid #ddd',
    },
    buttonGroup: {
      display: 'flex',
      justifyContent: 'space-between',
      marginTop: '16px',
      gap: '8px',
    },
    button: {
      padding: '8px 16px',
      borderRadius: '4px',
      border: 'none',
      fontSize: '14px',
      fontWeight: '500',
      cursor: 'pointer',
      transition: 'background-color 0.2s ease',
      flex: '1',
    },
    cancelButton: {
      backgroundColor: theme === 'dark' ? '#555' : '#eee',
      color: theme === 'dark' ? '#fff' : '#333',
    },
    submitButton: {
      backgroundColor: '#3a8eff',
      color: '#fff',
      opacity: loading ? '0.7' : '1',
    },
    amountContainer: {
      position: 'relative',
    },
    maxButton: {
      position: 'absolute',
      right: '8px',
      top: '8px',
      padding: '2px 6px',
      fontSize: '12px',
      backgroundColor: theme === 'dark' ? '#555' : '#eee',
      color: theme === 'dark' ? '#fff' : '#333',
      border: 'none',
      borderRadius: '4px',
      cursor: 'pointer',
    },
    statusContainer: {
      marginTop: '16px',
      padding: '12px',
      borderRadius: '4px',
      fontSize: '14px',
      backgroundColor: getStatusBackgroundColor(transactionStatus, theme),
      color: getStatusTextColor(transactionStatus, theme),
    },
    statusTitle: {
      fontWeight: '600',
      marginBottom: '4px',
    },
    statusMessage: {
      marginBottom: '8px',
    },
    transactionHash: {
      fontSize: '12px',
      wordBreak: 'break-all',
      padding: '4px 8px',
      backgroundColor:
        theme === 'dark' ? 'rgba(0, 0, 0, 0.2)' : 'rgba(0, 0, 0, 0.05)',
      borderRadius: '4px',
    },
  };

  // Get status background color
  function getStatusBackgroundColor(status, themeMode) {
    if (!status) return 'transparent';

    switch (status) {
      case 'pending':
        return themeMode === 'dark' ? '#554800' : '#fff8e1';
      case 'confirmed':
        return themeMode === 'dark' ? '#004d1a' : '#e6f7e6';
      case 'failed':
        return themeMode === 'dark' ? '#4d0000' : '#ffebee';
      default:
        return themeMode === 'dark' ? '#333' : '#f5f5f5';
    }
  }

  // Get status text color
  function getStatusTextColor(status, themeMode) {
    if (!status) return themeMode === 'dark' ? '#fff' : '#333';

    switch (status) {
      case 'pending':
        return themeMode === 'dark' ? '#ffeb3b' : '#ffc107';
      case 'confirmed':
        return themeMode === 'dark' ? '#4caf50' : '#2e7d32';
      case 'failed':
        return themeMode === 'dark' ? '#ff5252' : '#d32f2f';
      default:
        return themeMode === 'dark' ? '#fff' : '#333';
    }
  }

  // Get status title
  function getStatusTitle(status) {
    switch (status) {
      case 'pending':
        return 'Transaction Pending';
      case 'confirmed':
        return 'Transaction Confirmed';
      case 'failed':
        return 'Transaction Failed';
      default:
        return 'Transaction Status';
    }
  }

  // Get status message
  function getStatusMessage(status) {
    switch (status) {
      case 'pending':
        return 'Your transaction is being processed. This may take a few moments.';
      case 'confirmed':
        return 'Your transaction has been confirmed and added to the blockchain.';
      case 'failed':
        return 'Your transaction failed to process. Please try again.';
      default:
        return '';
    }
  }

  return (
    <div style={styles.container} className="wallet-transfer">
      <div style={styles.header}>
        <div style={styles.title}>Transfer WRF</div>
      </div>

      {error && <div style={styles.error}>{error}</div>}

      <form style={styles.form} onSubmit={handleSubmit}>
        <div style={styles.formGroup}>
          <label style={styles.label}>From</label>
          <input
            style={styles.input}
            type="text"
            value={walletAddress}
            disabled
          />
        </div>

        <div style={styles.formGroup}>
          <label style={styles.label}>To</label>
          <input
            style={styles.input}
            type="text"
            placeholder="WRF address of recipient"
            value={recipient}
            onChange={e => {
              setRecipient(e.target.value);
              validateAddressFormat(e.target.value);
            }}
            disabled={loading}
            required
          />
          {addressError && <div style={styles.error}>{addressError}</div>}
        </div>

        <div style={styles.formGroup}>
          <label style={styles.label}>Amount (WRF)</label>
          <div style={styles.amountContainer}>
            <input
              style={styles.input}
              type="number"
              step="0.000001"
              min="0.000001"
              placeholder="0.0"
              value={amount}
              onChange={e => {
                setAmount(e.target.value);
                validateAmount(e.target.value);
              }}
              disabled={loading}
              required
            />
            <button
              type="button"
              style={styles.maxButton}
              onClick={handleSetMaxAmount}
              disabled={loading || !fee}
            >
              MAX
            </button>
          </div>
          {amountError && <div style={styles.error}>{amountError}</div>}
        </div>

        <div style={styles.formGroup}>
          <label style={styles.label}>Memo (Optional)</label>
          <textarea
            style={styles.textarea}
            placeholder="Add a note to this transaction"
            value={memo}
            onChange={e => setMemo(e.target.value)}
            disabled={loading}
            maxLength={100}
          />
        </div>

        {amount > 0 && (
          <div style={styles.feeContainer}>
            <div style={styles.feeInfo}>
              <span>Network Fee:</span>
              <span style={styles.feeValue}>
                {estimatingFee ? 'Calculating...' : formatFee(fee)}
              </span>
            </div>

            {fee && amount && (
              <div style={styles.totalAmount}>
                <span>Total Amount:</span>
                <span>{getTotalAmount()} WRF</span>
              </div>
            )}
          </div>
        )}

        <div style={styles.buttonGroup}>
          <button
            type="button"
            style={{ ...styles.button, ...styles.cancelButton }}
            onClick={handleCancel}
            disabled={loading}
          >
            Cancel
          </button>

          <button
            type="submit"
            style={{ ...styles.button, ...styles.submitButton }}
            disabled={
              loading || !amount || !recipient || addressError || amountError
            }
          >
            {loading ? 'Sending...' : 'Send WRF'}
          </button>
        </div>
      </form>

      {transactionStatus && (
        <div style={styles.statusContainer}>
          <div style={styles.statusTitle}>
            {getStatusTitle(transactionStatus)}
          </div>
          <div style={styles.statusMessage}>
            {getStatusMessage(transactionStatus)}
          </div>

          {transactionHash && (
            <div style={styles.transactionHash}>
              Transaction ID: {transactionHash}
            </div>
          )}
        </div>
      )}
    </div>
  );
};

WalletTransfer.propTypes = {
  walletAddress: PropTypes.string.isRequired,
  onSuccess: PropTypes.func,
  onCancel: PropTypes.func,
  theme: PropTypes.oneOf(['light', 'dark']),
  maxAmount: PropTypes.number,
  validateAddress: PropTypes.bool,
};

export default WalletTransfer;
