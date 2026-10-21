# Wallet Management

Complete guide to creating, securing, and managing your Wrfcoin wallet for storing WRF tokens and
interacting with the blockchain.

## Creating Your First Wallet

### Using Wrfcoin Core Wallet

#### Step 1: Download and Install

```bash
# Download Wrfcoin Core
wget https://releases.wrfcoin.network/latest/wrfcoin-wallet.tar.gz
tar -xzf wrfcoin-wallet.tar.gz
sudo ./install.sh
```

#### Step 2: Create New Wallet

```bash
# Create a new wallet
wrfcoin-wallet create --name "my-main-wallet"

# This will generate:
# - 24-word recovery phrase
# - Private/public key pair
# - Wallet address
```

#### Step 3: Secure Your Recovery Phrase

**Critical**: Write down your 24-word recovery phrase and store it safely:

1. Write on paper (never store digitally)
2. Make multiple copies
3. Store in different secure locations
4. Consider using metal backup plates for fire/water resistance

### Using Hardware Wallets

#### Supported Hardware Wallets

- **Ledger Nano S/X**: Full Wrfcoin app support
- **Trezor Model T**: Native integration
- **SafePal S1**: Air-gapped security

#### Setup Process

1. **Initialize Hardware Wallet**

   ```bash
   # Connect your hardware wallet
   wrfcoin-wallet hw init --device ledger

   # Import existing wallet
   wrfcoin-wallet hw import --device ledger --wallet-id 0
   ```

2. **Verify Connection**

   ```bash
   # Check wallet status
   wrfcoin-wallet hw status

   # Display address
   wrfcoin-wallet hw address --verify
   ```

## Wallet Security Best Practices

### Multi-Signature Wallets

Create wallets requiring multiple signatures for enhanced security:

```bash
# Create 2-of-3 multisig wallet
wrfcoin-wallet multisig create \
  --threshold 2 \
  --participants 3 \
  --name "company-treasury"

# Add cosigners
wrfcoin-wallet multisig add-cosigner \
  --wallet "company-treasury" \
  --pubkey "02a1b2c3d4..."
```

### Cold Storage Setup

#### Creating an Offline Wallet

1. **Air-Gapped Computer Setup**

   - Use dedicated computer never connected to internet
   - Install Wrfcoin Core from verified offline installer
   - Generate wallet on offline machine

2. **Transaction Signing Process**

   ```bash
   # On online computer: Create unsigned transaction
   wrfcoin-wallet tx create \
     --to wrf1abc...xyz \
     --amount 100 \
     --output unsigned-tx.json

   # Transfer file to offline computer
   # On offline computer: Sign transaction
   wrfcoin-wallet tx sign \
     --input unsigned-tx.json \
     --output signed-tx.json

   # Transfer back and broadcast
   wrfcoin-wallet tx broadcast --file signed-tx.json
   ```

### Access Control

#### Password Protection

```bash
# Set wallet password
wrfcoin-wallet password set --wallet "my-wallet"

# Enable automatic lock
wrfcoin-wallet config set auto_lock_minutes 15

# Require password for transactions
wrfcoin-wallet config set require_password_for_tx true
```

#### Biometric Authentication

```bash
# Enable fingerprint unlock (supported systems)
wrfcoin-wallet biometric enable --method fingerprint

# Setup face recognition
wrfcoin-wallet biometric enable --method face
```

## Managing Multiple Wallets

### Wallet Organization

```bash
# Create wallets for different purposes
wrfcoin-wallet create --name "personal-spending"
wrfcoin-wallet create --name "mining-rewards"
wrfcoin-wallet create --name "staking-wallet"
wrfcoin-wallet create --name "trading-funds"

# List all wallets
wrfcoin-wallet list

# Set default wallet
wrfcoin-wallet default set "personal-spending"
```

### Account Management

```bash
# Create sub-accounts within wallet
wrfcoin-wallet account create \
  --wallet "personal-spending" \
  --name "daily-expenses"

wrfcoin-wallet account create \
  --wallet "personal-spending" \
  --name "savings"

# Generate new addresses
wrfcoin-wallet address new --account "daily-expenses"
```

## Transaction Management

### Sending Transactions

#### Basic Transfer

```bash
# Send WRF tokens
wrfcoin-wallet send \
  --to wrf1recipient...address \
  --amount 50.5 \
  --memo "Payment for weather data"

# Send with custom fee
wrfcoin-wallet send \
  --to wrf1recipient...address \
  --amount 50.5 \
  --fee 0.001
```

#### Batch Transactions

```bash
# Create batch payment file
cat > batch-payments.json << EOF
[
  {
    "to": "wrf1addr1...",
    "amount": "10.0",
    "memo": "Weather station reward"
  },
  {
    "to": "wrf1addr2...",
    "amount": "25.5",
    "memo": "Validator commission"
  }
]
EOF

# Execute batch payment
wrfcoin-wallet batch-send --file batch-payments.json
```

### Transaction History

```bash
# View transaction history
wrfcoin-wallet history --limit 50

# Export transaction history
wrfcoin-wallet history export \
  --format csv \
  --start-date 2024-01-01 \
  --end-date 2024-12-31 \
  --output transactions-2024.csv

# Search transactions
wrfcoin-wallet history search --memo "mining"
```

## Advanced Features

### Staking Integration

#### Delegate to Validators

```bash
# View available validators
wrfcoin-wallet staking validators

# Delegate tokens
wrfcoin-wallet staking delegate \
  --validator wrfvaloper1... \
  --amount 1000

# View delegations
wrfcoin-wallet staking delegations

# Claim rewards
wrfcoin-wallet staking claim-rewards
```

#### Running Your Own Validator

```bash
# Convert wallet for validator use
wrfcoin-wallet validator init \
  --moniker "My Weather Validator" \
  --commission-rate 0.05 \
  --min-self-delegation 1000

# Create validator
wrfcoin-wallet validator create \
  --stake 10000 \
  --website "https://myvalidator.com"
```

### DeFi Integration

#### Liquidity Provision

```bash
# Add liquidity to WRF/ETH pool
wrfcoin-wallet defi add-liquidity \
  --pool WRF-ETH \
  --amount-wrf 1000 \
  --amount-eth 2.5

# Remove liquidity
wrfcoin-wallet defi remove-liquidity \
  --pool WRF-ETH \
  --percentage 50
```

#### Yield Farming

```bash
# Stake LP tokens for rewards
wrfcoin-wallet defi farm stake \
  --pool WRF-ETH-LP \
  --amount 100

# Harvest farming rewards
wrfcoin-wallet defi farm harvest --pool WRF-ETH-LP
```

## Cross-Chain Operations

### Bridge Connections

```bash
# Connect to Ethereum bridge
wrfcoin-wallet bridge connect --chain ethereum

# Bridge WRF to Ethereum
wrfcoin-wallet bridge send \
  --to-chain ethereum \
  --to-address 0x1234... \
  --amount 500

# Check bridge status
wrfcoin-wallet bridge status --tx-hash abc123...
```

### Multi-Chain Portfolio

```bash
# View balances across all chains
wrfcoin-wallet portfolio overview

# Sync with external chains
wrfcoin-wallet portfolio sync

# Export portfolio report
wrfcoin-wallet portfolio export --format pdf
```

## Mobile Wallet Integration

### QR Code Operations

```bash
# Generate QR code for receiving
wrfcoin-wallet qr receive --amount 25.5

# Scan QR code for sending (requires camera)
wrfcoin-wallet qr send

# Generate payment request QR
wrfcoin-wallet qr payment-request \
  --amount 100 \
  --memo "Weather station payment"
```

### NFC Payments

```bash
# Enable NFC for payments
wrfcoin-wallet nfc enable

# Set daily NFC limit
wrfcoin-wallet nfc set-limit 50

# View NFC transaction log
wrfcoin-wallet nfc history
```

## Backup and Recovery

### Creating Backups

```bash
# Create encrypted wallet backup
wrfcoin-wallet backup create \
  --output wallet-backup-$(date +%Y%m%d).enc \
  --password

# Backup only keys
wrfcoin-wallet backup keys \
  --output keys-backup.json

# Create paper wallet
wrfcoin-wallet backup paper --include-qr
```

### Recovery Procedures

```bash
# Restore from seed phrase
wrfcoin-wallet restore \
  --method seed \
  --words "word1 word2 ... word24"

# Restore from backup file
wrfcoin-wallet restore \
  --method backup \
  --file wallet-backup.enc

# Restore from private key
wrfcoin-wallet restore \
  --method private-key \
  --key "5k..."
```

## Troubleshooting

### Common Issues

#### Sync Problems

```bash
# Force wallet sync
wrfcoin-wallet sync --force

# Reset wallet data (keeps keys)
wrfcoin-wallet reset --keep-keys

# Rebuild transaction index
wrfcoin-wallet rebuild-index
```

#### Balance Discrepancies

```bash
# Refresh balance
wrfcoin-wallet balance refresh

# Rescan blockchain
wrfcoin-wallet rescan --from-height 1000000

# Verify transaction status
wrfcoin-wallet tx verify --hash abc123...
```

#### Connection Issues

```bash
# Test network connectivity
wrfcoin-wallet network test

# Switch to different node
wrfcoin-wallet config set node_url "https://node2.wrfcoin.network"

# Reset network settings
wrfcoin-wallet network reset
```

## Monitoring and Analytics

### Portfolio Tracking

```bash
# Real-time portfolio value
wrfcoin-wallet portfolio value --currency USD

# Performance analytics
wrfcoin-wallet analytics performance --period 30d

# Tax reporting
wrfcoin-wallet tax-report generate --year 2024
```

### Alerts and Notifications

```bash
# Set up balance alerts
wrfcoin-wallet alerts balance \
  --threshold 100 \
  --type email \
  --contact user@example.com

# Price movement alerts
wrfcoin-wallet alerts price \
  --change-percentage 10 \
  --notification push

# Transaction confirmations
wrfcoin-wallet alerts transaction \
  --confirmations 6 \
  --method sms
```

## Integration with External Services

### Exchange Connections

```bash
# Connect to supported exchanges
wrfcoin-wallet exchange connect --provider binance

# Sync exchange balances
wrfcoin-wallet exchange sync

# Execute trades
wrfcoin-wallet exchange trade \
  --pair WRF/USDT \
  --type buy \
  --amount 1000
```

### DeFi Protocols

```bash
# Connect to lending protocols
wrfcoin-wallet defi lend \
  --protocol compound \
  --asset WRF \
  --amount 500

# Borrow against collateral
wrfcoin-wallet defi borrow \
  --protocol aave \
  --asset USDT \
  --amount 100
```

## Security Recommendations

### Regular Security Practices

1. **Update Software Regularly**

   ```bash
   # Check for updates
   wrfcoin-wallet update check

   # Update to latest version
   wrfcoin-wallet update install
   ```

2. **Monitor Unusual Activity**

   ```bash
   # Review recent transactions
   wrfcoin-wallet security review

   # Check for unauthorized access
   wrfcoin-wallet security audit
   ```

3. **Enable Security Features**

   ```bash
   # Two-factor authentication
   wrfcoin-wallet 2fa enable

   # Transaction signing requirements
   wrfcoin-wallet security set-signing-policy strict
   ```

### Emergency Procedures

```bash
# Freeze wallet (emergency)
wrfcoin-wallet emergency freeze

# Report compromised wallet
wrfcoin-wallet security report-compromise

# Transfer to new wallet
wrfcoin-wallet emergency transfer-all \
  --to-wallet "emergency-backup"
```

Remember: Your wallet security is paramount. Always verify addresses before sending transactions and
keep your recovery phrases secure and offline.
