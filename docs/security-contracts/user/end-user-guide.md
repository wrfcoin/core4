# End-User Guide - WRFCoin Security Contracts

## Overview

This guide provides step-by-step instructions for end users to safely interact with WRFCoin's
security contracts. These contracts protect significant value ($2M-$5M TVL) and require careful
attention to security procedures.

## Getting Started

### Prerequisites

#### Required Software

- **MetaMask Wallet**: Latest version (10.0+)
- **Hardware Wallet** (Recommended): Ledger or Trezor for high-value transactions
- **Modern Web Browser**: Chrome, Firefox, or Edge with up-to-date security features

#### Network Configuration

**WRFCoin Mainnet**

```
Network Name: WRFCoin Network
RPC URL: https://rpc.wrfcoin.network
Chain ID: 2024
Currency Symbol: WRF
Block Explorer URL: https://explorer.wrfcoin.network
```

**WRFCoin Testnet**

```
Network Name: WRFCoin Testnet
RPC URL: https://testnet-rpc.wrfcoin.network
Chain ID: 2025
Currency Symbol: tWRF
Block Explorer URL: https://testnet-explorer.wrfcoin.network
```

### Security Setup

#### 1. Wallet Security Best Practices

**MetaMask Security Configuration**

```
1. Enable hardware wallet connection
2. Set up strong password protection
3. Enable automatic lock after 5 minutes
4. Verify all transaction details before signing
5. Never share seed phrases or private keys
```

**Transaction Security Checklist**

- [ ] Verify contract addresses before interacting
- [ ] Double-check transaction amounts and recipients
- [ ] Use hardware wallet for transactions > $1,000
- [ ] Enable transaction notifications
- [ ] Keep software updated

#### 2. Contract Address Verification

**Always verify you're interacting with official contracts:**

**Mainnet Addresses** (verify on https://explorer.wrfcoin.network)

- SecureStakingRewardCalculator: `0x...` (TBD)
- SecureMultiSigWallet: `0x...` (TBD)

**Testnet Addresses** (verify on https://testnet-explorer.wrfcoin.network)

- SecureStakingRewardCalculator: `0x...` (TBD)
- SecureMultiSigWallet: `0x...` (TBD)

⚠️ **WARNING**: Never interact with contracts from unofficial sources or unverified addresses.

## Staking Rewards Guide

### Understanding Staking Rewards

#### Reward Components

1. **Base Reward**: Calculated from staked amount × base rate × duration
2. **Quality Multiplier**: Bonus based on carbon credit quality (50% - 200%)
3. **Vintage Bonus**: Additional bonus for recent vintage years (0% - 15%)
4. **Sustainability Bonus**: Bonus for sustainable practices (0% - 5%)
5. **Duration Bonus**: Long-term staking rewards (1% - 40%)

#### Quality Tiers

| Tier | Name     | Quality Score | Multiplier | Description              |
| ---- | -------- | ------------- | ---------- | ------------------------ |
| 1    | Premium  | 90-100        | 200%       | Highest quality credits  |
| 2    | High     | 75-89         | 150%       | High quality credits     |
| 3    | Standard | 50-74         | 100%       | Standard quality credits |
| 4    | Basic    | 25-49         | 75%        | Basic quality credits    |
| 5    | Low      | 0-24          | 50%        | Lower quality credits    |

### How to Calculate Expected Rewards

#### Using the WRFCoin Dashboard

1. **Navigate to Staking Calculator**

   ```
   https://app.wrfcoin.network/staking-calculator
   ```

2. **Enter Your Parameters**

   - Staking Amount: Enter the amount of tokens you plan to stake
   - Staking Duration: Select your intended staking period
   - Carbon Credit Type: Choose your carbon basket token

3. **Review Calculation Results**
   ```
   Base Reward: 1,000 WRF
   Quality Boost: +500 WRF (50% bonus)
   Vintage Bonus: +100 WRF (10% bonus)
   Duration Bonus: +200 WRF (20% bonus)
   ─────────────────────────
   Total Reward: 1,800 WRF
   Effective APY: 18.5%
   ```

#### Manual Calculation Example

**Example Scenario:**

- Staking Amount: 10,000 WRF
- Base Rate: 0.001 WRF per token per second
- Duration: 365 days (31,536,000 seconds)
- Quality Tier: High (150% multiplier)
- Vintage Year: 2024 (15% bonus)
- Sustainability: 5% bonus

**Step-by-Step Calculation:**

```
1. Base Reward = 10,000 × 0.001 × 31,536,000 / 1e18
   Base Reward = 315,360 WRF

2. Quality Boosted = 315,360 × 1.5 = 473,040 WRF

3. Vintage Bonus = 315,360 × 0.15 = 47,304 WRF

4. Sustainability Bonus = 315,360 × 0.05 = 15,768 WRF

5. Total Reward = 473,040 + 47,304 + 15,768 = 536,112 WRF
```

### Claiming Staking Rewards

#### Step-by-Step Process

1. **Access the Staking Dashboard**

   - Go to https://app.wrfcoin.network/staking
   - Connect your wallet
   - Verify network (WRFCoin Mainnet)

2. **Review Available Rewards**

   ```
   Your Staking Positions:
   ┌─────────────────────────────────────┐
   │ Farm: Carbon Credits Tier 1         │
   │ Staked: 10,000 WRF                 │
   │ Duration: 120 days                  │
   │ Available Rewards: 1,425 WRF       │
   │ Status: Ready to Claim             │
   └─────────────────────────────────────┘
   ```

3. **Initiate Claim Process**

   - Click "Claim Rewards" button
   - Review transaction details
   - Confirm gas fee estimates

4. **Sign Transaction**

   ```
   Transaction Details:
   Function: claimRewards
   Gas Limit: 250,000
   Gas Price: 20 gwei
   Estimated Fee: 0.005 ETH

   [Sign Transaction] [Cancel]
   ```

5. **Confirm Success**
   - Wait for transaction confirmation
   - Check wallet balance for reward tokens
   - Verify transaction on block explorer

#### Security Considerations

**Before Claiming:**

- [ ] Verify you're on the official WRFCoin website
- [ ] Check contract address matches official deployment
- [ ] Review reward amount for reasonableness
- [ ] Ensure sufficient gas fee balance

**During Transaction:**

- [ ] Read all transaction details carefully
- [ ] Verify function being called is correct
- [ ] Check recipient address is your wallet
- [ ] Confirm gas limit is reasonable (< 500,000)

## Multi-Signature Wallet Guide

### Understanding Multi-Sig Operations

#### How Multi-Sig Works

```
Step 1: Owner submits transaction proposal
Step 2: Required number of owners confirm with signatures
Step 3: Transaction executes automatically when threshold met
```

#### Transaction Types

1. **ETH Transfer**: Simple ETH transfers
2. **Token Transfer**: ERC20 token operations
3. **Contract Call**: Smart contract interactions
4. **Owner Management**: Adding/removing wallet owners
5. **Emergency Action**: Emergency response procedures

### Participating as a Multi-Sig Owner

#### Reviewing Pending Transactions

1. **Access Multi-Sig Dashboard**

   ```
   https://app.wrfcoin.network/multisig
   ```

2. **View Pending Transactions**

   ```
   Pending Transactions:
   ┌─────────────────────────────────────────────────┐
   │ TX #42: Transfer 1,000 ETH to Treasury         │
   │ Created: 2024-01-15 14:30 UTC                  │
   │ Expires: 2024-01-22 14:30 UTC                  │
   │ Confirmations: 2/3 required                    │
   │ Creator: 0x1234...5678                         │
   │ Status: ⏳ Awaiting Your Confirmation          │
   │ [View Details] [Confirm] [Reject]              │
   └─────────────────────────────────────────────────┘
   ```

3. **Review Transaction Details**

   ```
   Transaction Details:
   ─────────────────────
   ID: 42
   Type: ETH Transfer
   Recipient: 0xabcd...efgh (Treasury Contract)
   Amount: 1,000 ETH
   Description: "Monthly treasury allocation for development"

   Created by: 0x1234...5678 (Alice)
   Created at: Jan 15, 2024 14:30 UTC
   Expires at: Jan 22, 2024 14:30 UTC

   Confirmations:
   ✅ Alice (0x1234...5678) - Jan 15, 14:30
   ✅ Bob (0x5678...9abc) - Jan 15, 16:45
   ⏳ Awaiting: Charlie (0x9abc...def0)
   ```

#### Confirming Transactions

1. **Verify Transaction Legitimacy**

   - Check transaction details match expectations
   - Verify recipient address is correct and known
   - Confirm amount is reasonable
   - Review transaction description

2. **Generate Confirmation Signature**

   ```
   Click "Confirm Transaction"
   ↓
   MetaMask opens signature request
   ↓
   Review signature details:
   - Message type: EIP-712 structured data
   - Signing for: Transaction confirmation
   - Contract: SecureMultiSigWallet
   ↓
   Sign message
   ```

3. **Submit Confirmation**
   - Transaction will be submitted to blockchain
   - Gas fee will be charged for confirmation
   - Transaction may auto-execute if threshold reached

#### Security Best Practices for Owners

**Transaction Review Checklist:**

- [ ] Verify transaction was created by known owner
- [ ] Check recipient address is correct
- [ ] Confirm amount matches expectations
- [ ] Review transaction description for clarity
- [ ] Verify transaction type is appropriate
- [ ] Check expiration time is reasonable

**Red Flags - DO NOT CONFIRM:**

- ❌ Unfamiliar recipient addresses
- ❌ Unusually large amounts
- ❌ Vague or missing descriptions
- ❌ Transactions created by unknown addresses
- ❌ Rushed deadlines or pressure to confirm quickly

### Emergency Procedures

#### Recognizing Emergency Situations

**Immediate Action Required:**

- Suspicious transaction confirmations
- Unexpected emergency mode activation
- Unusual contract behavior
- Potential security breaches

#### Emergency Response Steps

1. **DO NOT PANIC**

   - Take time to assess the situation
   - Gather information before acting

2. **Contact Emergency Response Team**

   ```
   Emergency Contacts:
   - Security Team: security@wrfcoin.network
   - Emergency Hotline: +1-xxx-xxx-xxxx
   - Telegram: @WRFCoinEmergency
   ```

3. **Document the Incident**

   - Screenshot relevant information
   - Note transaction IDs and addresses
   - Record timeline of events

4. **Follow Official Communications**
   - Monitor official WRFCoin channels
   - Wait for verified instructions
   - Do not act on unofficial information

## Security Best Practices

### Wallet Security

#### Hardware Wallet Integration

**Setting Up Ledger with MetaMask:**

```
1. Connect Ledger device to computer
2. Open Ledger Live and update firmware
3. Install Ethereum app on Ledger
4. In MetaMask: Settings → Advanced → Use Ledger
5. Select accounts to import
6. Verify address on Ledger screen
```

**Transaction Signing with Hardware Wallet:**

```
1. Initiate transaction in web interface
2. Review details in MetaMask
3. Click "Sign" in MetaMask
4. Confirm transaction details on Ledger screen
5. Press both buttons to sign
```

#### Phishing Protection

**Common Phishing Indicators:**

- ❌ URLs with slight misspellings (wrfcoln.network)
- ❌ Unexpected popup wallet connections
- ❌ Urgent requests for immediate action
- ❌ Requests for seed phrases or private keys
- ❌ Unofficial social media links

**Protection Strategies:**

- ✅ Bookmark official URLs
- ✅ Always type URLs manually
- ✅ Verify SSL certificates
- ✅ Enable browser security features
- ✅ Use official mobile apps

### Transaction Security

#### Gas Fee Management

**Understanding Gas Fees:**

```
Gas Price Options:
┌─────────────────────────────────┐
│ Slow: 10 gwei (~10 minutes)    │
│ Standard: 20 gwei (~3 minutes) │
│ Fast: 35 gwei (~30 seconds)    │
└─────────────────────────────────┘
```

**Gas Fee Best Practices:**

- Use "Standard" gas for most transactions
- Use "Fast" for time-sensitive operations
- Never use extremely high gas prices unless urgent
- Monitor network congestion before transacting

#### Transaction Monitoring

**Using Block Explorers:**

1. Copy transaction hash from wallet
2. Go to https://explorer.wrfcoin.network
3. Paste hash in search box
4. Monitor confirmation status

**Transaction Statuses:**

```
⏳ Pending: Transaction submitted, awaiting confirmation
✅ Success: Transaction confirmed and successful
❌ Failed: Transaction confirmed but failed
⚠️ Dropped: Transaction removed from mempool
```

### Account Security

#### Multi-Factor Authentication

**Enable 2FA where available:**

- WRFCoin dashboard account
- Email accounts
- Hardware wallet PINs
- Backup recovery phrases

#### Recovery Procedures

**If Wallet is Compromised:**

1. **Immediate Actions:**

   - Stop all transactions
   - Transfer funds to new wallet
   - Change all related passwords

2. **Recovery Steps:**

   - Create new wallet with new seed phrase
   - Update all service connections
   - Report incident to security team

3. **Prevention:**
   - Regular security audits
   - Keep software updated
   - Use hardware wallets for large amounts

## Common Issues and Solutions

### Transaction Issues

#### Transaction Stuck or Pending

**Diagnosis:**

```
Possible Causes:
- Gas price too low
- Network congestion
- Nonce issues
- Insufficient balance
```

**Solutions:**

1. **Increase Gas Price:**

   - Use MetaMask "Speed Up" feature
   - Submit new transaction with higher gas

2. **Cancel Transaction:**

   - Send 0 ETH to yourself with same nonce
   - Use higher gas price than original

3. **Wait for Network:**
   - Monitor network status
   - Transaction may confirm when congestion clears

#### Transaction Failed

**Common Failure Reasons:**

```
- Insufficient gas limit
- Contract requirements not met
- Slippage tolerance exceeded
- Deadline expired
- Contract paused or in emergency mode
```

**Troubleshooting Steps:**

1. Check transaction on block explorer
2. Review error message details
3. Verify contract status
4. Adjust transaction parameters
5. Retry with corrected settings

### Interface Issues

#### Website Not Loading

**Troubleshooting:**

1. Clear browser cache and cookies
2. Disable browser extensions temporarily
3. Try different browser or incognito mode
4. Check internet connection
5. Verify official URL

#### Wallet Connection Issues

**Common Solutions:**

1. Refresh webpage
2. Disconnect and reconnect wallet
3. Switch networks and switch back
4. Restart browser
5. Update MetaMask extension

### Reward Calculation Issues

#### Unexpected Reward Amounts

**Verification Steps:**

1. Check all input parameters
2. Verify quality tier and multipliers
3. Review staking duration calculation
4. Confirm vintage year bonuses
5. Contact support if calculations seem incorrect

## Support and Resources

### Getting Help

#### Official Support Channels

**Primary Support:**

- Website: https://support.wrfcoin.network
- Email: support@wrfcoin.network
- Response Time: 24-48 hours

**Community Support:**

- Discord: https://discord.gg/wrfcoin
- Telegram: https://t.me/wrfcoin
- Reddit: https://reddit.com/r/wrfcoin

**Emergency Support:**

- Security Issues: security@wrfcoin.network
- Critical Bugs: critical@wrfcoin.network
- Emergency Hotline: +1-xxx-xxx-xxxx

#### Documentation Resources

**Technical Documentation:**

- Developer Docs: https://docs.wrfcoin.network
- API Reference: https://api-docs.wrfcoin.network
- Smart Contract ABIs: https://github.com/wrfcoin/contracts

**Educational Resources:**

- Beginner's Guide: https://learn.wrfcoin.network
- Video Tutorials: https://youtube.com/wrfcoin
- Webinar Series: https://webinars.wrfcoin.network

### Reporting Issues

#### Security Vulnerabilities

**Bug Bounty Program:**

- Submit security reports: security@wrfcoin.network
- PGP Key: Available at https://wrfcoin.network/pgp
- Rewards: Up to $50,000 for critical vulnerabilities

#### General Issues

**When Reporting Issues Include:**

- Detailed description of the problem
- Steps to reproduce the issue
- Screenshots or screen recordings
- Browser and wallet version information
- Transaction hashes (if applicable)
- Network used (mainnet/testnet)

### Educational Resources

#### Video Tutorials

1. **"Getting Started with WRFCoin Staking"**

   - Duration: 15 minutes
   - Covers: Wallet setup, staking basics, reward calculations

2. **"Multi-Sig Wallet Security Best Practices"**

   - Duration: 20 minutes
   - Covers: Owner responsibilities, transaction review, emergency procedures

3. **"Advanced Security for High-Value Users"**
   - Duration: 30 minutes
   - Covers: Hardware wallets, cold storage, operational security

#### Written Guides

- **Staking Strategy Guide**: Optimizing rewards and managing risk
- **Security Checklist**: Comprehensive security verification steps
- **Troubleshooting Guide**: Solutions for common issues

---

_This end-user guide provides comprehensive instructions for safely interacting with WRFCoin's
security contracts while maintaining the highest security standards._
