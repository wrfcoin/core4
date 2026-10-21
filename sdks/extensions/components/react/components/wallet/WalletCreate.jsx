import React, { useState } from 'react';
import PropTypes from 'prop-types';
import { useWrfcoinContext } from '../../context/WrfcoinContext';

/**
 * WalletCreate component for creating a new wallet or importing an existing one
 *
 * This component provides functionality to create a new wallet or import an existing
 * wallet using a seed phrase, private key, or JSON file.
 */
const WalletCreate = ({
  onSuccess,
  onCancel,
  theme = 'light',
  defaultTab = 'create',
}) => {
  const { sdk } = useWrfcoinContext();
  const [activeTab, setActiveTab] = useState(defaultTab);
  const [walletName, setWalletName] = useState('');
  const [password, setPassword] = useState('');
  const [confirmPassword, setConfirmPassword] = useState('');
  const [seedPhrase, setSeedPhrase] = useState('');
  const [privateKey, setPrivateKey] = useState('');
  const [walletFile, setWalletFile] = useState(null);
  const [walletFilePassword, setWalletFilePassword] = useState('');
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState(null);
  const [walletCreated, setWalletCreated] = useState(false);
  const [createdWalletInfo, setCreatedWalletInfo] = useState(null);
  const [generatedSeedPhrase, setGeneratedSeedPhrase] = useState('');
  const [seedPhraseConfirmed, setSeedPhraseConfirmed] = useState(false);
  const [seedPhraseWords, setSeedPhraseWords] = useState([]);
  const [seedPhraseInput, setSeedPhraseInput] = useState([]);

  // Handle tab change
  const handleTabChange = tab => {
    setActiveTab(tab);
    setError(null);
  };

  // Validate password
  const validatePassword = () => {
    if (!password) {
      setError('Password is required');
      return false;
    }

    if (password.length < 8) {
      setError('Password must be at least 8 characters long');
      return false;
    }

    if (password !== confirmPassword) {
      setError('Passwords do not match');
      return false;
    }

    return true;
  };

  // Validate seed phrase
  const validateSeedPhrase = () => {
    if (!seedPhrase) {
      setError('Seed phrase is required');
      return false;
    }

    const words = seedPhrase.trim().split(/\s+/);

    if (words.length !== 12 && words.length !== 24) {
      setError('Seed phrase must be 12 or 24 words');
      return false;
    }

    return true;
  };

  // Validate private key
  const validatePrivateKey = () => {
    if (!privateKey) {
      setError('Private key is required');
      return false;
    }

    // Basic validation (should be replaced with actual validation logic)
    if (privateKey.length < 32) {
      setError('Invalid private key format');
      return false;
    }

    return true;
  };

  // Handle create wallet submission
  const handleCreateWallet = async e => {
    e.preventDefault();

    if (!validatePassword()) {
      return;
    }

    try {
      setLoading(true);
      setError(null);

      // Create wallet
      const wallet = await sdk.createWallet({
        name: walletName || 'My Wrfcoin Wallet',
        password: password,
      });

      setCreatedWalletInfo(wallet);
      setGeneratedSeedPhrase(wallet.seedPhrase);

      // Parse seed phrase into words for confirmation UI
      const words = wallet.seedPhrase.trim().split(/\s+/);
      setSeedPhraseWords(words);

      // Set seedPhraseInput to array of same length but empty strings
      setSeedPhraseInput(Array(words.length).fill(''));

      setWalletCreated(true);
    } catch (err) {
      console.error('Error creating wallet:', err);
      setError('Failed to create wallet. Please try again later.');
    } finally {
      setLoading(false);
    }
  };

  // Handle seed phrase import submission
  const handleImportSeedPhrase = async e => {
    e.preventDefault();

    if (!validateSeedPhrase() || !validatePassword()) {
      return;
    }

    try {
      setLoading(true);
      setError(null);

      // Import wallet from seed phrase
      const wallet = await sdk.importWalletFromSeedPhrase({
        seedPhrase: seedPhrase,
        name: walletName || 'Imported Wallet',
        password: password,
      });

      if (onSuccess && typeof onSuccess === 'function') {
        onSuccess(wallet);
      }
    } catch (err) {
      console.error('Error importing wallet from seed phrase:', err);
      setError(
        'Failed to import wallet. Please verify your seed phrase and try again.'
      );
    } finally {
      setLoading(false);
    }
  };

  // Handle private key import submission
  const handleImportPrivateKey = async e => {
    e.preventDefault();

    if (!validatePrivateKey() || !validatePassword()) {
      return;
    }

    try {
      setLoading(true);
      setError(null);

      // Import wallet from private key
      const wallet = await sdk.importWalletFromPrivateKey({
        privateKey: privateKey,
        name: walletName || 'Imported Wallet',
        password: password,
      });

      if (onSuccess && typeof onSuccess === 'function') {
        onSuccess(wallet);
      }
    } catch (err) {
      console.error('Error importing wallet from private key:', err);
      setError(
        'Failed to import wallet. Please verify your private key and try again.'
      );
    } finally {
      setLoading(false);
    }
  };

  // Handle wallet file import submission
  const handleImportWalletFile = async e => {
    e.preventDefault();

    if (!walletFile) {
      setError('Please select a wallet file');
      return;
    }

    try {
      setLoading(true);
      setError(null);

      // Read file contents
      const fileReader = new FileReader();
      fileReader.readAsText(walletFile);

      fileReader.onload = async () => {
        try {
          // Import wallet from JSON
          const wallet = await sdk.importWalletFromJSON({
            json: fileReader.result,
            password: walletFilePassword,
            name: walletName || 'Imported Wallet',
          });

          if (onSuccess && typeof onSuccess === 'function') {
            onSuccess(wallet);
          }
        } catch (err) {
          console.error('Error importing wallet from JSON:', err);
          setError(
            'Failed to import wallet. Please verify your file and password and try again.'
          );
          setLoading(false);
        }
      };

      fileReader.onerror = () => {
        setError('Failed to read wallet file');
        setLoading(false);
      };
    } catch (err) {
      console.error('Error importing wallet from file:', err);
      setError(
        'Failed to import wallet. Please verify your file and try again.'
      );
      setLoading(false);
    }
  };

  // Handle file input change
  const handleFileChange = e => {
    if (e.target.files.length > 0) {
      setWalletFile(e.target.files[0]);
    }
  };

  // Handle confirmation of seed phrase
  const handleConfirmSeedPhrase = () => {
    // Check if all words match the original seed phrase
    const isValid = seedPhraseWords.every((word, index) => {
      return word.toLowerCase() === seedPhraseInput[index].toLowerCase();
    });

    if (!isValid) {
      setError('Seed phrase does not match. Please try again.');
      return;
    }

    setSeedPhraseConfirmed(true);
  };

  // Handle specific seed phrase word input change
  const handleSeedWordChange = (index, value) => {
    const newSeedPhraseInput = [...seedPhraseInput];
    newSeedPhraseInput[index] = value;
    setSeedPhraseInput(newSeedPhraseInput);
  };

  // Handle final confirmation and completion
  const handleFinishSetup = () => {
    if (onSuccess && typeof onSuccess === 'function') {
      onSuccess(createdWalletInfo);
    }
  };

  // Handle cancel button click
  const handleCancel = () => {
    if (onCancel && typeof onCancel === 'function') {
      onCancel();
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
      marginBottom: '8px',
    },
    description: {
      fontSize: '14px',
      color: theme === 'dark' ? '#aaa' : '#777',
      marginBottom: '16px',
    },
    tabs: {
      display: 'flex',
      borderBottom: theme === 'dark' ? '1px solid #444' : '1px solid #ddd',
      marginBottom: '16px',
    },
    tab: isActive => ({
      padding: '8px 16px',
      cursor: 'pointer',
      fontSize: '14px',
      fontWeight: isActive ? '600' : '400',
      color: isActive
        ? theme === 'dark'
          ? '#fff'
          : '#333'
        : theme === 'dark'
          ? '#aaa'
          : '#777',
      borderBottom: isActive ? '2px solid #3a8eff' : 'none',
    }),
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
      fontSize: '14px',
      marginTop: '4px',
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
    seedPhraseContainer: {
      backgroundColor: theme === 'dark' ? '#222' : '#f9f9f9',
      padding: '16px',
      borderRadius: '4px',
      marginBottom: '16px',
    },
    seedPhraseHeading: {
      fontSize: '16px',
      fontWeight: '600',
      marginBottom: '8px',
    },
    seedPhraseWarning: {
      color: theme === 'dark' ? '#ffcc00' : '#f57c00',
      fontSize: '14px',
      marginBottom: '16px',
    },
    seedPhraseDisplay: {
      padding: '12px',
      backgroundColor: theme === 'dark' ? '#333' : '#f5f5f5',
      borderRadius: '4px',
      fontSize: '16px',
      fontFamily: 'monospace',
      wordBreak: 'break-word',
      marginBottom: '16px',
      border: theme === 'dark' ? '1px solid #444' : '1px solid #ddd',
    },
    seedPhraseGrid: {
      display: 'grid',
      gridTemplateColumns: 'repeat(3, 1fr)',
      gap: '8px',
      marginBottom: '16px',
    },
    seedPhraseWord: {
      display: 'flex',
      alignItems: 'center',
      padding: '8px',
      backgroundColor: theme === 'dark' ? '#333' : '#f5f5f5',
      borderRadius: '4px',
      fontSize: '14px',
    },
    seedPhraseNumber: {
      marginRight: '8px',
      fontSize: '12px',
      color: theme === 'dark' ? '#aaa' : '#777',
      width: '16px',
      textAlign: 'right',
    },
    seedPhraseConfirmation: {
      marginTop: '16px',
      padding: '16px',
      backgroundColor: theme === 'dark' ? '#222' : '#f9f9f9',
      borderRadius: '4px',
    },
    seedPhraseInputGrid: {
      display: 'grid',
      gridTemplateColumns: 'repeat(3, 1fr)',
      gap: '8px',
      marginBottom: '16px',
    },
    seedPhraseInputContainer: {
      display: 'flex',
      alignItems: 'center',
    },
    fileInputButton: {
      padding: '8px 12px',
      borderRadius: '4px',
      border: theme === 'dark' ? '1px solid #555' : '1px solid #ddd',
      backgroundColor: theme === 'dark' ? '#444' : '#f5f5f5',
      color: theme === 'dark' ? '#fff' : '#333',
      fontSize: '14px',
      cursor: 'pointer',
      textAlign: 'center',
    },
    fileInputLabel: {
      fontSize: '14px',
      color: theme === 'dark' ? '#aaa' : '#777',
      marginTop: '4px',
    },
    successContainer: {
      display: 'flex',
      flexDirection: 'column',
      alignItems: 'center',
      padding: '16px',
    },
    successIcon: {
      fontSize: '36px',
      color: theme === 'dark' ? '#8aff8a' : '#4caf50',
      marginBottom: '8px',
    },
    successTitle: {
      fontSize: '18px',
      fontWeight: '600',
      marginBottom: '8px',
    },
    successDescription: {
      fontSize: '14px',
      color: theme === 'dark' ? '#aaa' : '#777',
      marginBottom: '16px',
      textAlign: 'center',
    },
  };

  // Render confirm seed phrase UI
  const renderConfirmSeedPhrase = () => {
    return (
      <div style={styles.form}>
        <div style={styles.seedPhraseContainer}>
          <div style={styles.seedPhraseHeading}>
            Confirm your recovery phrase
          </div>
          <div style={styles.description}>
            Please enter your recovery phrase in the correct order to confirm
            you've saved it properly.
          </div>

          <div style={styles.seedPhraseInputGrid}>
            {seedPhraseWords.map((_, index) => (
              <div key={index} style={styles.seedPhraseInputContainer}>
                <div style={styles.seedPhraseNumber}>{index + 1}</div>
                <input
                  style={styles.input}
                  type="text"
                  value={seedPhraseInput[index]}
                  onChange={e => handleSeedWordChange(index, e.target.value)}
                  placeholder={`Word ${index + 1}`}
                />
              </div>
            ))}
          </div>

          {error && <div style={styles.error}>{error}</div>}
        </div>

        <div style={styles.buttonGroup}>
          <button
            type="button"
            style={{ ...styles.button, ...styles.cancelButton }}
            onClick={handleCancel}
          >
            Cancel
          </button>

          <button
            type="button"
            style={{ ...styles.button, ...styles.submitButton }}
            onClick={handleConfirmSeedPhrase}
            disabled={seedPhraseInput.some(word => !word)}
          >
            Confirm Recovery Phrase
          </button>
        </div>
      </div>
    );
  };

  // Render seed phrase display UI
  const renderSeedPhraseDisplay = () => {
    return (
      <div style={styles.form}>
        <div style={styles.seedPhraseContainer}>
          <div style={styles.seedPhraseHeading}>Your Recovery Phrase</div>
          <div style={styles.seedPhraseWarning}>
            Warning: Never share your recovery phrase! Anyone with these words
            can access and take control of your wallet.
          </div>

          <div style={styles.seedPhraseDisplay}>{generatedSeedPhrase}</div>

          <div style={styles.description}>
            Write down these words in the exact order shown above and store them
            in a secure location. This is the only way to recover your wallet if
            you lose access.
          </div>

          <div style={styles.seedPhraseGrid}>
            {seedPhraseWords.map((word, index) => (
              <div key={index} style={styles.seedPhraseWord}>
                <span style={styles.seedPhraseNumber}>{index + 1}</span>
                <span>{word}</span>
              </div>
            ))}
          </div>
        </div>

        <div style={styles.buttonGroup}>
          <button
            type="button"
            style={{ ...styles.button, ...styles.cancelButton }}
            onClick={handleCancel}
          >
            Cancel
          </button>

          <button
            type="button"
            style={{ ...styles.button, ...styles.submitButton }}
            onClick={() => setSeedPhraseConfirmed(false)}
          >
            I've Written It Down
          </button>
        </div>
      </div>
    );
  };

  // Render success UI
  const renderSuccess = () => {
    return (
      <div style={styles.successContainer}>
        <div style={styles.successIcon}>✓</div>
        <div style={styles.successTitle}>Wallet Successfully Created!</div>
        <div style={styles.successDescription}>
          Your new wallet has been created and is ready to use. Remember to keep
          your recovery phrase safe and secure.
        </div>

        <button
          style={{ ...styles.button, ...styles.submitButton, width: '100%' }}
          onClick={handleFinishSetup}
        >
          Go to My Wallet
        </button>
      </div>
    );
  };

  // Render create wallet form UI
  const renderCreateWalletForm = () => {
    return (
      <form style={styles.form} onSubmit={handleCreateWallet}>
        <div style={styles.formGroup}>
          <label style={styles.label}>Wallet Name (Optional)</label>
          <input
            style={styles.input}
            type="text"
            placeholder="My Wrfcoin Wallet"
            value={walletName}
            onChange={e => setWalletName(e.target.value)}
            disabled={loading}
          />
        </div>

        <div style={styles.formGroup}>
          <label style={styles.label}>Password</label>
          <input
            style={styles.input}
            type="password"
            placeholder="Create a strong password"
            value={password}
            onChange={e => setPassword(e.target.value)}
            disabled={loading}
            required
          />
        </div>

        <div style={styles.formGroup}>
          <label style={styles.label}>Confirm Password</label>
          <input
            style={styles.input}
            type="password"
            placeholder="Confirm your password"
            value={confirmPassword}
            onChange={e => setConfirmPassword(e.target.value)}
            disabled={loading}
            required
          />
        </div>

        {error && <div style={styles.error}>{error}</div>}

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
            disabled={loading || !password || !confirmPassword}
          >
            {loading ? 'Creating...' : 'Create Wallet'}
          </button>
        </div>
      </form>
    );
  };

  // Render import from seed phrase form UI
  const renderImportSeedPhraseForm = () => {
    return (
      <form style={styles.form} onSubmit={handleImportSeedPhrase}>
        <div style={styles.formGroup}>
          <label style={styles.label}>Wallet Name (Optional)</label>
          <input
            style={styles.input}
            type="text"
            placeholder="Imported Wallet"
            value={walletName}
            onChange={e => setWalletName(e.target.value)}
            disabled={loading}
          />
        </div>

        <div style={styles.formGroup}>
          <label style={styles.label}>Recovery Phrase</label>
          <textarea
            style={styles.textarea}
            placeholder="Enter your 12 or 24 word recovery phrase, separated by spaces"
            value={seedPhrase}
            onChange={e => setSeedPhrase(e.target.value)}
            disabled={loading}
            required
          />
        </div>

        <div style={styles.formGroup}>
          <label style={styles.label}>New Password</label>
          <input
            style={styles.input}
            type="password"
            placeholder="Create a strong password"
            value={password}
            onChange={e => setPassword(e.target.value)}
            disabled={loading}
            required
          />
        </div>

        <div style={styles.formGroup}>
          <label style={styles.label}>Confirm Password</label>
          <input
            style={styles.input}
            type="password"
            placeholder="Confirm your password"
            value={confirmPassword}
            onChange={e => setConfirmPassword(e.target.value)}
            disabled={loading}
            required
          />
        </div>

        {error && <div style={styles.error}>{error}</div>}

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
            disabled={loading || !seedPhrase || !password || !confirmPassword}
          >
            {loading ? 'Importing...' : 'Import Wallet'}
          </button>
        </div>
      </form>
    );
  };

  // Render import from private key form UI
  const renderImportPrivateKeyForm = () => {
    return (
      <form style={styles.form} onSubmit={handleImportPrivateKey}>
        <div style={styles.formGroup}>
          <label style={styles.label}>Wallet Name (Optional)</label>
          <input
            style={styles.input}
            type="text"
            placeholder="Imported Wallet"
            value={walletName}
            onChange={e => setWalletName(e.target.value)}
            disabled={loading}
          />
        </div>

        <div style={styles.formGroup}>
          <label style={styles.label}>Private Key</label>
          <input
            style={styles.input}
            type="password"
            placeholder="Enter your private key"
            value={privateKey}
            onChange={e => setPrivateKey(e.target.value)}
            disabled={loading}
            required
          />
        </div>

        <div style={styles.formGroup}>
          <label style={styles.label}>New Password</label>
          <input
            style={styles.input}
            type="password"
            placeholder="Create a strong password"
            value={password}
            onChange={e => setPassword(e.target.value)}
            disabled={loading}
            required
          />
        </div>

        <div style={styles.formGroup}>
          <label style={styles.label}>Confirm Password</label>
          <input
            style={styles.input}
            type="password"
            placeholder="Confirm your password"
            value={confirmPassword}
            onChange={e => setConfirmPassword(e.target.value)}
            disabled={loading}
            required
          />
        </div>

        {error && <div style={styles.error}>{error}</div>}

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
            disabled={loading || !privateKey || !password || !confirmPassword}
          >
            {loading ? 'Importing...' : 'Import Wallet'}
          </button>
        </div>
      </form>
    );
  };

  // Render import from file form UI
  const renderImportFileForm = () => {
    return (
      <form style={styles.form} onSubmit={handleImportWalletFile}>
        <div style={styles.formGroup}>
          <label style={styles.label}>Wallet Name (Optional)</label>
          <input
            style={styles.input}
            type="text"
            placeholder="Imported Wallet"
            value={walletName}
            onChange={e => setWalletName(e.target.value)}
            disabled={loading}
          />
        </div>

        <div style={styles.formGroup}>
          <label style={styles.label}>Wallet File</label>
          <label style={styles.fileInputButton}>
            <input
              type="file"
              onChange={handleFileChange}
              accept=".json"
              style={{ display: 'none' }}
              disabled={loading}
            />
            {walletFile ? 'File Selected' : 'Choose File'}
          </label>
          {walletFile && (
            <div style={styles.fileInputLabel}>{walletFile.name}</div>
          )}
        </div>

        <div style={styles.formGroup}>
          <label style={styles.label}>Wallet File Password</label>
          <input
            style={styles.input}
            type="password"
            placeholder="Enter the password for this wallet file"
            value={walletFilePassword}
            onChange={e => setWalletFilePassword(e.target.value)}
            disabled={loading}
            required
          />
        </div>

        {error && <div style={styles.error}>{error}</div>}

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
            disabled={loading || !walletFile || !walletFilePassword}
          >
            {loading ? 'Importing...' : 'Import Wallet'}
          </button>
        </div>
      </form>
    );
  };

  // Determine which form to render
  const renderContent = () => {
    if (walletCreated) {
      if (seedPhraseConfirmed) {
        return renderSuccess();
      } else if (!seedPhraseConfirmed && generatedSeedPhrase) {
        return renderSeedPhraseDisplay();
      } else {
        return renderConfirmSeedPhrase();
      }
    }

    switch (activeTab) {
      case 'create':
        return renderCreateWalletForm();
      case 'seedphrase':
        return renderImportSeedPhraseForm();
      case 'privatekey':
        return renderImportPrivateKeyForm();
      case 'file':
        return renderImportFileForm();
      default:
        return renderCreateWalletForm();
    }
  };

  return (
    <div style={styles.container} className="wallet-create">
      <div style={styles.header}>
        <div style={styles.title}>
          {walletCreated ? 'Wallet Setup' : 'Create or Import Wallet'}
        </div>
        {!walletCreated && (
          <div style={styles.description}>
            Create a new Wrfcoin wallet or import an existing one using your
            recovery phrase, private key, or wallet file.
          </div>
        )}
      </div>

      {!walletCreated && (
        <div style={styles.tabs}>
          <div
            style={styles.tab(activeTab === 'create')}
            onClick={() => handleTabChange('create')}
          >
            Create New
          </div>
          <div
            style={styles.tab(activeTab === 'seedphrase')}
            onClick={() => handleTabChange('seedphrase')}
          >
            Seed Phrase
          </div>
          <div
            style={styles.tab(activeTab === 'privatekey')}
            onClick={() => handleTabChange('privatekey')}
          >
            Private Key
          </div>
          <div
            style={styles.tab(activeTab === 'file')}
            onClick={() => handleTabChange('file')}
          >
            File
          </div>
        </div>
      )}

      {renderContent()}
    </div>
  );
};

WalletCreate.propTypes = {
  onSuccess: PropTypes.func,
  onCancel: PropTypes.func,
  theme: PropTypes.oneOf(['light', 'dark']),
  defaultTab: PropTypes.oneOf(['create', 'seedphrase', 'privatekey', 'file']),
};

export default WalletCreate;
