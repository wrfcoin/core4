# Basic Smart Contract Interaction

Learn how to interact with WRFCoin's core smart contracts for climate data, governance, and DeFi
operations.

## What You'll Learn

- Core WRFCoin smart contract architecture
- Reading data from contracts using view functions
- Writing data with state-changing transactions
- Event listening and real-time contract monitoring
- Gas optimization strategies
- Contract interaction patterns and best practices

## Prerequisites

- Completed [SDK Integration tutorials](../sdk-integration/)
- Basic understanding of smart contracts and blockchain
- Familiarity with JavaScript or Python

---

## 1. WRFCoin Smart Contract Ecosystem

### Core Contract Overview

```javascript
// contracts/core-contracts.js
const { WrfcoinClient } = require('@wrfcoin/sdk');

class WrfcoinContracts {
  constructor(client) {
    this.client = client;
    this.contracts = {};
    this.contractAddresses = {
      // Core Protocol Contracts
      WRFToken: '0x742d35Cc6C4C73006cF3D741eE919dDD13F7f0C1',
      ClimateDataRegistry: '0x123f45C3D9E7B890F1234567890ABCDEF1234567',
      WeatherOracle: '0x456789ABCDEF1234567890ABCDEF1234567890AB',

      // Governance Contracts
      WRFGovernance: '0x789ABCDEF1234567890ABCDEF1234567890ABCDE',
      ProposalManager: '0xABCDEF1234567890ABCDEF1234567890ABCDEF12',
      VotingPower: '0xDEF1234567890ABCDEF1234567890ABCDEF123456',

      // DeFi Contracts
      ClimateAMM: '0x234567890ABCDEF1234567890ABCDEF1234567890',
      CarbonCreditToken: '0x567890ABCDEF1234567890ABCDEF1234567890AB',
      StakingRewards: '0x890ABCDEF1234567890ABCDEF1234567890ABCDE',

      // NFT and Gaming
      ComputationalNFT: '0xBCDEF1234567890ABCDEF1234567890ABCDEF123',
      ClimateAchievements: '0xEF1234567890ABCDEF1234567890ABCDEF123456',
    };
  }

  async initialize() {
    console.log('🔗 Initializing WRFCoin smart contracts');

    try {
      // Load all contract instances
      for (const [name, address] of Object.entries(this.contractAddresses)) {
        this.contracts[name] = await this.client.contract.load(address);
        console.log(`✅ Loaded ${name} contract at ${address}`);
      }

      // Verify contract connectivity
      await this._verifyContracts();

      console.log('🎉 All contracts initialized successfully');
    } catch (error) {
      console.error('❌ Contract initialization failed:', error);
      throw error;
    }
  }

  async _verifyContracts() {
    // Verify core contracts are responsive
    const tokenName = await this.contracts.WRFToken.name();
    const dataRegistryVersion = await this.contracts.ClimateDataRegistry.version();

    console.log(`📝 Token name: ${tokenName}`);
    console.log(`📊 Data registry version: ${dataRegistryVersion}`);
  }

  // Getter methods for contract instances
  getWRFToken() {
    return this.contracts.WRFToken;
  }
  getClimateDataRegistry() {
    return this.contracts.ClimateDataRegistry;
  }
  getWeatherOracle() {
    return this.contracts.WeatherOracle;
  }
  getGovernance() {
    return this.contracts.WRFGovernance;
  }
  getClimateAMM() {
    return this.contracts.ClimateAMM;
  }
  getCarbonCreditToken() {
    return this.contracts.CarbonCreditToken;
  }
  getComputationalNFT() {
    return this.contracts.ComputationalNFT;
  }
}

module.exports = WrfcoinContracts;
```

---

## 2. Reading Contract Data

### Basic View Functions

```javascript
// examples/reading-contract-data.js
const WrfcoinContracts = require('../contracts/core-contracts');

class ContractDataReader {
  constructor(wrfcoinClient) {
    this.client = wrfcoinClient;
    this.contracts = null;
  }

  async initialize() {
    this.contracts = new WrfcoinContracts(this.client);
    await this.contracts.initialize();
  }

  async readTokenInformation() {
    console.log('📊 Reading WRF Token Information\n');

    const token = this.contracts.getWRFToken();

    try {
      // Basic token information
      const name = await token.name();
      const symbol = await token.symbol();
      const decimals = await token.decimals();
      const totalSupply = await token.totalSupply();

      console.log(`Token Name: ${name}`);
      console.log(`Symbol: ${symbol}`);
      console.log(`Decimals: ${decimals}`);
      console.log(`Total Supply: ${this.formatTokenAmount(totalSupply, decimals)} ${symbol}`);

      // Advanced token metrics
      const stakingInfo = await this._getStakingInformation(token);
      const distributionInfo = await this._getDistributionInformation(token);

      return {
        name,
        symbol,
        decimals,
        totalSupply,
        stakingInfo,
        distributionInfo,
      };
    } catch (error) {
      console.error('❌ Failed to read token information:', error);
      throw error;
    }
  }

  async _getStakingInformation(token) {
    try {
      const totalStaked = await token.totalStaked();
      const stakingRewards = await token.totalStakingRewards();
      const currentAPY = await token.getCurrentAPY();

      console.log(`\n💰 Staking Information:`);
      console.log(`Total Staked: ${this.formatTokenAmount(totalStaked, 18)} WRF`);
      console.log(`Staking Rewards Pool: ${this.formatTokenAmount(stakingRewards, 18)} WRF`);
      console.log(`Current APY: ${currentAPY / 100}%`);

      return { totalStaked, stakingRewards, currentAPY };
    } catch (error) {
      console.log(`⚠️  Staking information not available: ${error.message}`);
      return null;
    }
  }

  async _getDistributionInformation(token) {
    try {
      const circulatingSupply = await token.circulatingSupply();
      const lockedTokens = await token.totalLocked();
      const burnedTokens = await token.totalBurned();

      console.log(`\n🏦 Distribution Information:`);
      console.log(`Circulating Supply: ${this.formatTokenAmount(circulatingSupply, 18)} WRF`);
      console.log(`Locked Tokens: ${this.formatTokenAmount(lockedTokens, 18)} WRF`);
      console.log(`Burned Tokens: ${this.formatTokenAmount(burnedTokens, 18)} WRF`);

      return { circulatingSupply, lockedTokens, burnedTokens };
    } catch (error) {
      console.log(`⚠️  Distribution information not available: ${error.message}`);
      return null;
    }
  }

  async readClimateData() {
    console.log('\n🌡️  Reading Climate Data Registry\n');

    const registry = this.contracts.getClimateDataRegistry();

    try {
      // Registry statistics
      const totalSubmissions = await registry.getTotalSubmissions();
      const validatedSubmissions = await registry.getValidatedSubmissions();
      const totalRewards = await registry.getTotalRewardsDistributed();

      console.log(`Total Data Submissions: ${totalSubmissions.toString()}`);
      console.log(`Validated Submissions: ${validatedSubmissions.toString()}`);
      console.log(`Total Rewards Distributed: ${this.formatTokenAmount(totalRewards, 18)} WRF`);

      // Recent submissions
      const recentSubmissions = await this._getRecentClimateData(registry, 10);

      // Data quality metrics
      const qualityMetrics = await this._getDataQualityMetrics(registry);

      return {
        totalSubmissions,
        validatedSubmissions,
        totalRewards,
        recentSubmissions,
        qualityMetrics,
      };
    } catch (error) {
      console.error('❌ Failed to read climate data:', error);
      throw error;
    }
  }

  async _getRecentClimateData(registry, count = 10) {
    try {
      console.log(`\n📋 Recent Climate Data Submissions (Last ${count}):`);

      const submissions = [];
      const totalSubmissions = await registry.getTotalSubmissions();
      const startIndex = Math.max(0, totalSubmissions - count);

      for (let i = startIndex; i < totalSubmissions; i++) {
        const submission = await registry.getSubmissionByIndex(i);
        submissions.push({
          id: submission.id.toString(),
          timestamp: new Date(submission.timestamp * 1000).toISOString(),
          location: {
            lat: submission.latitude / 1e6, // Convert from scaled integer
            lng: submission.longitude / 1e6,
          },
          dataType: submission.dataType,
          validated: submission.validated,
          qualityScore: submission.qualityScore / 100, // Convert from percentage
          reward: this.formatTokenAmount(submission.reward, 18),
        });

        console.log(
          `  ${i + 1}. ${submission.dataType} at (${submissions[i].location.lat}, ${submissions[i].location.lng}) - Quality: ${submissions[i].qualityScore} - ${submissions[i].validated ? '✅' : '⏳'}`
        );
      }

      return submissions;
    } catch (error) {
      console.log(`⚠️  Could not fetch recent submissions: ${error.message}`);
      return [];
    }
  }

  async _getDataQualityMetrics(registry) {
    try {
      const averageQuality = await registry.getAverageQualityScore();
      const topContributors = await registry.getTopContributors(5);
      const dataTypeStats = await registry.getDataTypeStatistics();

      console.log(`\n📈 Data Quality Metrics:`);
      console.log(`Average Quality Score: ${averageQuality / 100}%`);

      console.log(`\n🏆 Top Contributors:`);
      for (let i = 0; i < topContributors.length; i++) {
        const contributor = topContributors[i];
        console.log(
          `  ${i + 1}. ${contributor.address.substring(0, 10)}... - ${contributor.submissions} submissions`
        );
      }

      return { averageQuality, topContributors, dataTypeStats };
    } catch (error) {
      console.log(`⚠️  Quality metrics not available: ${error.message}`);
      return null;
    }
  }

  async readGovernanceInformation() {
    console.log('\n🏛️  Reading Governance Information\n');

    const governance = this.contracts.getGovernance();

    try {
      // Active proposals
      const activeProposals = await governance.getActiveProposals();
      const totalProposals = await governance.getTotalProposals();
      const votingPower = await governance.getTotalVotingPower();

      console.log(`Total Proposals: ${totalProposals.toString()}`);
      console.log(`Active Proposals: ${activeProposals.length}`);
      console.log(`Total Voting Power: ${this.formatTokenAmount(votingPower, 18)} WRF`);

      // Proposal details
      console.log(`\n📋 Active Proposals:`);
      for (let i = 0; i < activeProposals.length; i++) {
        const proposal = activeProposals[i];
        const details = await governance.getProposal(proposal);

        console.log(`  ${i + 1}. ${details.title}`);
        console.log(`     Status: ${details.status}`);
        console.log(`     Votes For: ${this.formatTokenAmount(details.votesFor, 18)} WRF`);
        console.log(`     Votes Against: ${this.formatTokenAmount(details.votesAgainst, 18)} WRF`);
        console.log(`     Ends: ${new Date(details.endTime * 1000).toLocaleDateString()}`);
      }

      return { activeProposals, totalProposals, votingPower };
    } catch (error) {
      console.error('❌ Failed to read governance information:', error);
      throw error;
    }
  }

  async readDeFiMetrics() {
    console.log('\n💱 Reading DeFi Protocol Metrics\n');

    const amm = this.contracts.getClimateAMM();
    const carbonToken = this.contracts.getCarbonCreditToken();

    try {
      // AMM metrics
      const totalValueLocked = await amm.getTotalValueLocked();
      const tradingVolume24h = await amm.getTradingVolume24h();
      const totalTrades = await amm.getTotalTrades();

      console.log(`🏦 Climate AMM:`);
      console.log(`  Total Value Locked: $${this.formatNumber(totalValueLocked)} USD`);
      console.log(`  24h Trading Volume: $${this.formatNumber(tradingVolume24h)} USD`);
      console.log(`  Total Trades: ${totalTrades.toString()}`);

      // Carbon credit metrics
      const totalCarbonCredits = await carbonToken.totalSupply();
      const retiredCredits = await carbonToken.totalRetired();
      const creditPrice = await amm.getCarbonCreditPrice();

      console.log(`\n🌱 Carbon Credits:`);
      console.log(`  Total Credits: ${this.formatTokenAmount(totalCarbonCredits, 18)} tons CO2`);
      console.log(`  Retired Credits: ${this.formatTokenAmount(retiredCredits, 18)} tons CO2`);
      console.log(`  Current Price: $${this.formatNumber(creditPrice)} per ton`);

      return {
        amm: { totalValueLocked, tradingVolume24h, totalTrades },
        carbonCredits: { totalCarbonCredits, retiredCredits, creditPrice },
      };
    } catch (error) {
      console.error('❌ Failed to read DeFi metrics:', error);
      throw error;
    }
  }

  async checkUserBalances(userAddress) {
    console.log(`\n👤 Reading User Balances for ${userAddress}\n`);

    try {
      const token = this.contracts.getWRFToken();
      const carbonToken = this.contracts.getCarbonCreditToken();

      // WRF token balances
      const wrfBalance = await token.balanceOf(userAddress);
      const stakedBalance = await token.stakedBalanceOf(userAddress);
      const pendingRewards = await token.pendingRewards(userAddress);

      console.log(`💰 WRF Balances:`);
      console.log(`  Available: ${this.formatTokenAmount(wrfBalance, 18)} WRF`);
      console.log(`  Staked: ${this.formatTokenAmount(stakedBalance, 18)} WRF`);
      console.log(`  Pending Rewards: ${this.formatTokenAmount(pendingRewards, 18)} WRF`);

      // Carbon credit balances
      const carbonBalance = await carbonToken.balanceOf(userAddress);

      console.log(`\n🌱 Carbon Credits:`);
      console.log(`  Balance: ${this.formatTokenAmount(carbonBalance, 18)} tons CO2`);

      // Governance participation
      const votingPower = await this.contracts.getGovernance().getVotingPower(userAddress);
      const proposalsVoted = await this.contracts.getGovernance().getProposalsVotedBy(userAddress);

      console.log(`\n🗳️  Governance:`);
      console.log(`  Voting Power: ${this.formatTokenAmount(votingPower, 18)} WRF`);
      console.log(`  Proposals Voted: ${proposalsVoted.length}`);

      return {
        wrf: { available: wrfBalance, staked: stakedBalance, pendingRewards },
        carbonCredits: carbonBalance,
        governance: { votingPower, proposalsVoted: proposalsVoted.length },
      };
    } catch (error) {
      console.error('❌ Failed to read user balances:', error);
      throw error;
    }
  }

  // Utility functions
  formatTokenAmount(amount, decimals) {
    return (parseFloat(amount.toString()) / Math.pow(10, decimals)).toLocaleString(undefined, {
      minimumFractionDigits: 2,
      maximumFractionDigits: 6,
    });
  }

  formatNumber(num) {
    return parseFloat(num.toString()).toLocaleString(undefined, {
      minimumFractionDigits: 0,
      maximumFractionDigits: 2,
    });
  }
}

// Usage example
async function demonstrateContractReading() {
  const { WrfcoinClient } = require('@wrfcoin/sdk');

  try {
    // Initialize client
    const client = new WrfcoinClient({
      network: 'testnet',
    });

    // Create reader
    const reader = new ContractDataReader(client);
    await reader.initialize();

    // Read various contract data
    console.log('='.repeat(60));
    console.log('📚 WRFCoin Smart Contract Data Reader');
    console.log('='.repeat(60));

    await reader.readTokenInformation();
    await reader.readClimateData();
    await reader.readGovernanceInformation();
    await reader.readDeFiMetrics();

    // Check specific user (replace with actual address)
    const userAddress = '0x742d35Cc6C4C73006cF3D741eE919dDD13F7f0C1';
    await reader.checkUserBalances(userAddress);

    console.log('\n✅ Contract data reading completed successfully!');
  } catch (error) {
    console.error('❌ Contract reading failed:', error);
  }
}

// Run the demonstration
if (require.main === module) {
  demonstrateContractReading();
}

module.exports = ContractDataReader;
```

---

## 3. Writing to Contracts

### State-Changing Transactions

```javascript
// examples/writing-contract-data.js
const WrfcoinContracts = require('../contracts/core-contracts');

class ContractWriter {
  constructor(wrfcoinClient, wallet) {
    this.client = wrfcoinClient;
    this.wallet = wallet;
    this.contracts = null;
  }

  async initialize() {
    this.contracts = new WrfcoinContracts(this.client);
    await this.contracts.initialize();
  }

  async submitWeatherData(weatherData) {
    console.log('🌡️  Submitting Weather Data to Blockchain\n');

    const registry = this.contracts.getClimateDataRegistry();

    try {
      // Validate weather data
      this._validateWeatherData(weatherData);

      // Prepare transaction data
      const submissionData = {
        latitude: Math.round(weatherData.location.lat * 1e6), // Scale for integer storage
        longitude: Math.round(weatherData.location.lng * 1e6),
        temperature: Math.round(weatherData.temperature * 100), // 2 decimal precision
        humidity: Math.round(weatherData.humidity * 100),
        pressure: Math.round(weatherData.pressure * 100),
        timestamp: Math.floor(Date.now() / 1000),
        source: weatherData.source || 'sdk_submission',
        metadata: JSON.stringify(weatherData.metadata || {}),
      };

      console.log('📊 Submitting data:', submissionData);

      // Estimate gas
      const gasEstimate = await registry.estimateGas.submitWeatherData(submissionData, {
        from: this.wallet.address,
      });

      console.log(`⛽ Estimated gas: ${gasEstimate.toString()}`);

      // Submit transaction
      const tx = await registry.submitWeatherData(submissionData, {
        from: this.wallet.address,
        gasLimit: Math.ceil(gasEstimate * 1.2), // 20% buffer
        gasPrice: await this.client.getGasPrice(),
      });

      console.log(`📤 Transaction sent: ${tx.hash}`);
      console.log('⏳ Waiting for confirmation...');

      // Wait for confirmation
      const receipt = await tx.wait(2); // Wait for 2 confirmations

      console.log(`✅ Transaction confirmed in block ${receipt.blockNumber}`);
      console.log(`💰 Gas used: ${receipt.gasUsed.toString()}`);

      // Extract submission ID from events
      const submissionEvent = receipt.events.find(event => event.event === 'WeatherDataSubmitted');
      const submissionId = submissionEvent?.args?.submissionId?.toString();

      if (submissionId) {
        console.log(`🆔 Submission ID: ${submissionId}`);

        // Check for immediate validation
        const validationStatus = await this._checkValidationStatus(submissionId);
        console.log(`🔍 Validation status: ${validationStatus}`);
      }

      return {
        transactionHash: tx.hash,
        blockNumber: receipt.blockNumber,
        gasUsed: receipt.gasUsed.toString(),
        submissionId: submissionId,
      };
    } catch (error) {
      console.error('❌ Weather data submission failed:', error);
      throw error;
    }
  }

  _validateWeatherData(data) {
    const required = ['location', 'temperature', 'humidity', 'pressure'];

    for (const field of required) {
      if (!(field in data)) {
        throw new Error(`Missing required field: ${field}`);
      }
    }

    if (!data.location.lat || !data.location.lng) {
      throw new Error('Invalid location coordinates');
    }

    // Range validation
    if (data.temperature < -100 || data.temperature > 60) {
      throw new Error('Temperature out of valid range (-100°C to 60°C)');
    }

    if (data.humidity < 0 || data.humidity > 100) {
      throw new Error('Humidity out of valid range (0% to 100%)');
    }

    if (data.pressure < 800 || data.pressure > 1100) {
      throw new Error('Pressure out of valid range (800 to 1100 hPa)');
    }
  }

  async _checkValidationStatus(submissionId) {
    try {
      const registry = this.contracts.getClimateDataRegistry();
      const submission = await registry.getSubmission(submissionId);

      if (submission.validated) {
        return 'validated';
      } else if (submission.rejected) {
        return 'rejected';
      } else {
        return 'pending';
      }
    } catch (error) {
      return 'unknown';
    }
  }

  async stakeTokens(amount) {
    console.log(`💰 Staking ${amount} WRF Tokens\n`);

    const token = this.contracts.getWRFToken();

    try {
      // Convert amount to Wei (18 decimals)
      const amountWei = this.client.utils.parseEther(amount.toString());

      // Check current balance
      const balance = await token.balanceOf(this.wallet.address);
      if (balance.lt(amountWei)) {
        throw new Error(
          `Insufficient balance. Available: ${this.client.utils.formatEther(balance)} WRF`
        );
      }

      // Check current allowance
      const stakingContract = await token.stakingContract();
      const allowance = await token.allowance(this.wallet.address, stakingContract);

      // Approve if necessary
      if (allowance.lt(amountWei)) {
        console.log('📝 Approving token spending...');

        const approveTx = await token.approve(stakingContract, amountWei, {
          from: this.wallet.address,
        });

        await approveTx.wait();
        console.log('✅ Approval confirmed');
      }

      // Stake tokens
      console.log('🔒 Staking tokens...');

      const stakeTx = await token.stake(amountWei, {
        from: this.wallet.address,
      });

      console.log(`📤 Stake transaction sent: ${stakeTx.hash}`);

      const receipt = await stakeTx.wait();
      console.log(`✅ Staking confirmed in block ${receipt.blockNumber}`);

      // Get updated staking information
      const stakedBalance = await token.stakedBalanceOf(this.wallet.address);
      const currentAPY = await token.getCurrentAPY();

      console.log(`🎉 Successfully staked ${amount} WRF!`);
      console.log(`📊 Total staked: ${this.client.utils.formatEther(stakedBalance)} WRF`);
      console.log(`📈 Current APY: ${currentAPY / 100}%`);

      return {
        transactionHash: stakeTx.hash,
        blockNumber: receipt.blockNumber,
        stakedAmount: amount,
        totalStaked: this.client.utils.formatEther(stakedBalance),
        currentAPY: currentAPY / 100,
      };
    } catch (error) {
      console.error('❌ Token staking failed:', error);
      throw error;
    }
  }

  async createGovernanceProposal(proposal) {
    console.log('🏛️  Creating Governance Proposal\n');

    const governance = this.contracts.getGovernance();

    try {
      // Validate proposal data
      this._validateProposal(proposal);

      // Check proposer eligibility
      const minProposalThreshold = await governance.proposalThreshold();
      const voterPower = await governance.getVotingPower(this.wallet.address);

      if (voterPower.lt(minProposalThreshold)) {
        throw new Error(
          `Insufficient voting power. Required: ${this.client.utils.formatEther(minProposalThreshold)} WRF`
        );
      }

      // Prepare proposal data
      const proposalData = {
        title: proposal.title,
        description: proposal.description,
        targets: proposal.targets || [],
        values: proposal.values || [],
        calldatas: proposal.calldatas || [],
        startTime: proposal.startTime || Math.floor(Date.now() / 1000) + 3600, // Start in 1 hour
        endTime: proposal.endTime || Math.floor(Date.now() / 1000) + 7 * 24 * 3600, // End in 7 days
      };

      console.log('📝 Creating proposal:', proposalData.title);

      // Submit proposal
      const proposalTx = await governance.propose(
        proposalData.targets,
        proposalData.values,
        proposalData.calldatas,
        proposalData.description,
        {
          from: this.wallet.address,
        }
      );

      console.log(`📤 Proposal transaction sent: ${proposalTx.hash}`);

      const receipt = await proposalTx.wait();
      console.log(`✅ Proposal created in block ${receipt.blockNumber}`);

      // Extract proposal ID from events
      const proposalEvent = receipt.events.find(event => event.event === 'ProposalCreated');
      const proposalId = proposalEvent?.args?.proposalId?.toString();

      if (proposalId) {
        console.log(`🆔 Proposal ID: ${proposalId}`);
        console.log(
          `🗳️  Voting will start at: ${new Date(proposalData.startTime * 1000).toLocaleString()}`
        );
        console.log(
          `⏰ Voting will end at: ${new Date(proposalData.endTime * 1000).toLocaleString()}`
        );
      }

      return {
        transactionHash: proposalTx.hash,
        blockNumber: receipt.blockNumber,
        proposalId: proposalId,
        startTime: proposalData.startTime,
        endTime: proposalData.endTime,
      };
    } catch (error) {
      console.error('❌ Proposal creation failed:', error);
      throw error;
    }
  }

  _validateProposal(proposal) {
    if (!proposal.title || proposal.title.length < 10) {
      throw new Error('Proposal title must be at least 10 characters');
    }

    if (!proposal.description || proposal.description.length < 50) {
      throw new Error('Proposal description must be at least 50 characters');
    }

    // Additional validation for executable proposals
    if (proposal.targets && proposal.targets.length > 0) {
      if (!proposal.values || proposal.values.length !== proposal.targets.length) {
        throw new Error('Values array must match targets array length');
      }

      if (!proposal.calldatas || proposal.calldatas.length !== proposal.targets.length) {
        throw new Error('Calldatas array must match targets array length');
      }
    }
  }

  async voteOnProposal(proposalId, support, reason = '') {
    console.log(`🗳️  Voting on Proposal ${proposalId}\n`);

    const governance = this.contracts.getGovernance();

    try {
      // Validate proposal exists and is active
      const proposal = await governance.getProposal(proposalId);
      const currentTime = Math.floor(Date.now() / 1000);

      if (currentTime < proposal.startTime) {
        throw new Error('Voting has not started yet');
      }

      if (currentTime > proposal.endTime) {
        throw new Error('Voting has ended');
      }

      // Check if already voted
      const hasVoted = await governance.hasVoted(proposalId, this.wallet.address);
      if (hasVoted) {
        throw new Error('Already voted on this proposal');
      }

      // Get voting power
      const votingPower = await governance.getVotingPower(this.wallet.address);
      if (votingPower.eq(0)) {
        throw new Error('No voting power');
      }

      console.log(`💪 Your voting power: ${this.client.utils.formatEther(votingPower)} WRF`);
      console.log(`📊 Vote: ${support ? 'FOR' : 'AGAINST'}`);
      if (reason) console.log(`💬 Reason: ${reason}`);

      // Submit vote
      const voteTx = await governance.castVoteWithReason(
        proposalId,
        support ? 1 : 0, // 1 = For, 0 = Against
        reason,
        {
          from: this.wallet.address,
        }
      );

      console.log(`📤 Vote transaction sent: ${voteTx.hash}`);

      const receipt = await voteTx.wait();
      console.log(`✅ Vote confirmed in block ${receipt.blockNumber}`);

      // Get updated proposal state
      const updatedProposal = await governance.getProposal(proposalId);

      console.log(`📈 Current vote tally:`);
      console.log(`  For: ${this.client.utils.formatEther(updatedProposal.forVotes)} WRF`);
      console.log(`  Against: ${this.client.utils.formatEther(updatedProposal.againstVotes)} WRF`);

      return {
        transactionHash: voteTx.hash,
        blockNumber: receipt.blockNumber,
        votingPower: this.client.utils.formatEther(votingPower),
        currentTally: {
          forVotes: this.client.utils.formatEther(updatedProposal.forVotes),
          againstVotes: this.client.utils.formatEther(updatedProposal.againstVotes),
        },
      };
    } catch (error) {
      console.error('❌ Voting failed:', error);
      throw error;
    }
  }

  async tradeCarbonCredits(amount, action = 'buy') {
    console.log(`💱 ${action.toUpperCase()} ${amount} Carbon Credits\n`);

    const amm = this.contracts.getClimateAMM();
    const carbonToken = this.contracts.getCarbonCreditToken();
    const wrfToken = this.contracts.getWRFToken();

    try {
      const amountWei = this.client.utils.parseEther(amount.toString());

      if (action === 'buy') {
        // Get current price
        const price = await amm.getCarbonCreditPrice();
        const totalCost = amountWei.mul(price).div(this.client.utils.parseEther('1'));

        console.log(`💰 Price: ${this.client.utils.formatEther(price)} WRF per credit`);
        console.log(`💸 Total cost: ${this.client.utils.formatEther(totalCost)} WRF`);

        // Check WRF balance
        const wrfBalance = await wrfToken.balanceOf(this.wallet.address);
        if (wrfBalance.lt(totalCost)) {
          throw new Error(
            `Insufficient WRF balance. Need: ${this.client.utils.formatEther(totalCost)} WRF`
          );
        }

        // Approve WRF spending
        const allowance = await wrfToken.allowance(this.wallet.address, amm.address);
        if (allowance.lt(totalCost)) {
          console.log('📝 Approving WRF spending...');
          const approveTx = await wrfToken.approve(amm.address, totalCost, {
            from: this.wallet.address,
          });
          await approveTx.wait();
          console.log('✅ Approval confirmed');
        }

        // Buy carbon credits
        const buyTx = await amm.buyCarbonCredits(amountWei, {
          from: this.wallet.address,
        });

        console.log(`📤 Buy transaction sent: ${buyTx.hash}`);
        const receipt = await buyTx.wait();
        console.log(`✅ Purchase confirmed in block ${receipt.blockNumber}`);

        return {
          action: 'buy',
          amount: amount,
          cost: this.client.utils.formatEther(totalCost),
          transactionHash: buyTx.hash,
        };
      } else if (action === 'sell') {
        // Check carbon credit balance
        const carbonBalance = await carbonToken.balanceOf(this.wallet.address);
        if (carbonBalance.lt(amountWei)) {
          throw new Error(
            `Insufficient carbon credits. Available: ${this.client.utils.formatEther(carbonBalance)}`
          );
        }

        // Approve carbon credit spending
        const allowance = await carbonToken.allowance(this.wallet.address, amm.address);
        if (allowance.lt(amountWei)) {
          console.log('📝 Approving carbon credit spending...');
          const approveTx = await carbonToken.approve(amm.address, amountWei, {
            from: this.wallet.address,
          });
          await approveTx.wait();
          console.log('✅ Approval confirmed');
        }

        // Sell carbon credits
        const sellTx = await amm.sellCarbonCredits(amountWei, {
          from: this.wallet.address,
        });

        console.log(`📤 Sell transaction sent: ${sellTx.hash}`);
        const receipt = await sellTx.wait();
        console.log(`✅ Sale confirmed in block ${receipt.blockNumber}`);

        return {
          action: 'sell',
          amount: amount,
          transactionHash: sellTx.hash,
        };
      }
    } catch (error) {
      console.error(`❌ Carbon credit ${action} failed:`, error);
      throw error;
    }
  }
}

// Usage example
async function demonstrateContractWriting() {
  const { WrfcoinClient } = require('@wrfcoin/sdk');

  try {
    // Initialize client and wallet
    const client = new WrfcoinClient({
      network: 'testnet',
    });

    // Load wallet (in production, use secure key management)
    const wallet = await client.wallet.fromMnemonic(process.env.WALLET_MNEMONIC);

    // Create writer
    const writer = new ContractWriter(client, wallet);
    await writer.initialize();

    console.log('='.repeat(60));
    console.log('✍️  WRFCoin Smart Contract Writer');
    console.log('='.repeat(60));

    // Example weather data submission
    const weatherData = {
      location: { lat: 40.7128, lng: -74.006 },
      temperature: 23.5,
      humidity: 65.2,
      pressure: 1013.25,
      source: 'tutorial_example',
      metadata: { station_id: 'NYC_001', quality: 'high' },
    };

    await writer.submitWeatherData(weatherData);

    // Example token staking
    await writer.stakeTokens(100); // Stake 100 WRF

    // Example governance proposal
    const proposal = {
      title: 'Increase Data Validation Rewards',
      description:
        'This proposal suggests increasing the rewards for data validation to incentivize more community participation in quality control processes. The increased rewards will help ensure high-quality climate data submissions.',
    };

    await writer.createGovernanceProposal(proposal);

    // Example voting (replace with actual proposal ID)
    // await writer.voteOnProposal('1', true, 'This will improve data quality');

    // Example carbon credit trading
    await writer.tradeCarbonCredits(10, 'buy'); // Buy 10 carbon credits

    console.log('\n✅ Contract writing demonstration completed!');
  } catch (error) {
    console.error('❌ Contract writing failed:', error);
  }
}

// Run the demonstration
if (require.main === module) {
  demonstrateContractWriting();
}

module.exports = ContractWriter;
```

---

## What You've Learned

✅ **Contract Architecture** - Understanding WRFCoin's smart contract ecosystem  
✅ **Reading Contract Data** - Using view functions to query blockchain state  
✅ **Writing Transactions** - State-changing operations with proper validation  
✅ **Gas Optimization** - Efficient transaction patterns and cost management  
✅ **Event Handling** - Processing transaction receipts and contract events  
✅ **Error Management** - Robust error handling for contract interactions

## Next Steps

1. **[Contract Deployment](./02-contract-deployment.md)** - Deploy your own contracts
2. **[Advanced Patterns](./03-advanced-contract-patterns.md)** - Complex interaction patterns
3. **[DeFi Integration](./04-defi-integration.md)** - Building financial applications
4. **[NFT Development](./05-nft-development.md)** - Creating climate NFTs

---

**🎉 Congratulations!** You've mastered basic smart contract interaction with WRFCoin. You're now
ready to build sophisticated blockchain applications!
