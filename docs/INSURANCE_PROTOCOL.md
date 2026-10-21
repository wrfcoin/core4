# Carbon Credit Insurance Protocol

## Overview

The Carbon Credit Insurance Protocol provides comprehensive protection against risks associated with
carbon credit projects, including project failure, verification issues, and regulatory risks. Built
on top of the existing WRF carbon credit infrastructure, it offers automated risk assessment, claims
processing, and governance mechanisms.

## Architecture

### Core Components

1. **CarbonCreditInsurance.sol** - Main insurance contract managing policies and claims
2. **InsurancePool.sol** - Capital pool management for insurance reserves
3. **InsuranceGovernance.sol** - Governance system for dispute resolution
4. **Quality & Volatility Oracles** - Risk assessment data sources

### Key Features

- **Multi-Type Coverage**: Project failure, verification risks, regulatory risks, delivery
  guarantees, invalidation cover
- **Automated Risk Assessment**: Oracle-based risk scoring and premium calculation
- **Capital Pool Management**: Decentralized capital provision with rewards
- **Governance & Disputes**: On-chain governance for complex claims
- **Claims Processing**: Automated processing for simple claims, governance for complex ones

## Insurance Coverage Types

### 1. Project Failure Coverage (`PROJECT_FAILURE`)

Protects against:

- Technical project failures
- Natural disasters affecting projects
- Project abandonment
- Operational inefficiencies

**Example**: A renewable energy project fails to generate expected credits due to equipment
malfunction.

### 2. Verification Risk Coverage (`VERIFICATION_RISK`)

Protects against:

- Monitoring and verification errors
- Fraudulent verification activities
- Changes in verification methodologies
- Data integrity issues

**Example**: Credits are invalidated due to errors in the monitoring methodology.

### 3. Regulatory Risk Coverage (`REGULATORY_RISK`)

Protects against:

- Policy changes affecting credit validity
- Political instability
- Registry standard changes
- Government intervention

**Example**: A government policy change makes certain types of carbon credits invalid.

### 4. Delivery Guarantee Coverage (`DELIVERY_GUARANTEE`)

Protects against:

- Failure to deliver promised credits
- Delays in credit issuance
- Registry processing issues

**Example**: A project fails to deliver credits by the agreed delivery date.

### 5. Invalidation Cover (`INVALIDATION_COVER`)

Protects against:

- Registry invalidation of credits
- Retroactive cancellation
- Standard compliance issues

**Example**: Previously issued credits are retroactively invalidated by the registry.

## How It Works

### 1. Policy Creation

```solidity
function createPolicy(
    uint256 tokenId,           // Carbon credit token to insure
    uint256 coveredAmount,     // Amount of credits to cover (kg CO2e)
    CoverageType[] coverageTypes, // Types of coverage desired
    uint256 duration,          // Policy duration in seconds
    string terms               // IPFS hash of policy terms
) external returns (uint256 policyId)
```

**Process**:

1. User selects carbon credit token and coverage amount
2. System assesses risk using oracle data
3. Premium is calculated based on risk factors
4. Capital is reserved in insurance pool
5. Policy becomes active upon premium payment

### 2. Risk Assessment

The protocol uses a comprehensive risk assessment model:

```solidity
struct RiskAssessment {
    uint256 projectRisk;       // Technical and operational risks (0-100)
    uint256 verificationRisk;  // Verification and monitoring risks (0-100)
    uint256 regulatoryRisk;    // Regulatory and political risks (0-100)
    uint256 marketRisk;        // Market and demand risks (0-100)
    uint256 overallRisk;       // Weighted overall risk score (0-100)
    uint256 qualityScore;      // From QualityOracle (0-1000)
    uint256 volatilityScore;   // From VolatilityOracle (0-10000)
    uint256 timestamp;
    string assessmentMethod;   // "automated", "manual", "hybrid"
}
```

**Risk Factors**:

- **Project Type**: Different project types have different base risk levels
- **Vintage**: Older projects may have higher risks
- **Registry Standard**: VCS, Gold Standard, etc. have different risk profiles
- **Quality Score**: Real-time quality assessment from oracles
- **Market Volatility**: Price volatility indicates market confidence

### 3. Premium Calculation

Premium calculation follows this formula:

```
Premium = (BasePremium × Duration × RiskMultiplier × QualityAdjustment × VolatilityAdjustment × CoverageAmount) / BASIS_POINTS
```

**Components**:

- **Base Premium**: Coverage-type specific rates (e.g., 3% for project failure)
- **Risk Multiplier**: Based on risk level (0.5x to 4x)
- **Quality Adjustment**: Better quality = lower premium
- **Volatility Adjustment**: Higher volatility = higher premium

### 4. Claims Processing

#### Simple Claims (Automated)

For straightforward claims, the system uses oracle data for validation:

```solidity
function submitClaim(
    uint256 policyId,
    CoverageType claimType,
    uint256 claimedAmount,
    string evidence
) external returns (uint256 claimId)
```

**Automated Assessment**:

- Project failure: Quality score drops below threshold
- Verification risk: Oracle data freshness/confidence issues
- Market validation through volatility data

#### Complex Claims (Governance)

Large claims or regulatory issues require governance:

1. Stakeholders stake WRF tokens for voting power
2. Proposals created for disputed claims
3. Voting period (7 days) with time-weighted voting power
4. Execution delay (2 days) for successful proposals

## Capital Pool Management

### Capital Providers

Anyone can become a capital provider by depositing WRF tokens:

```solidity
function depositCapital(uint256 amount) external returns (uint256 shares)
```

**Benefits**:

- Earn rewards from premiums (5% annual base rate)
- Participate in protocol governance
- Diversified risk exposure across all policies

### Pool Mechanics

- **Shares**: Proportional ownership of pool assets
- **Reserves**: Capital reserved for active policies
- **Solvency**: Minimum 120% solvency ratio required
- **Withdrawals**: 2-day delay for large withdrawals

## Governance System

### Voting Power

Voting power is determined by:

- Amount of WRF tokens staked
- Staking duration (up to 2x multiplier after 1 year)

### Proposal Types

1. **Claim Disputes** - Appeal rejected claims
2. **Policy Terms Updates** - Modify policy parameters
3. **Premium Adjustments** - Update risk assessment models
4. **Coverage Expansion** - Add new coverage types
5. **Risk Model Updates** - Improve risk assessment algorithms

### Voting Process

1. **Proposal Creation**: Requires minimum stake
2. **Voting Period**: 7 days for community input
3. **Quorum**: 40% of total staked tokens must participate
4. **Approval**: 51% support required
5. **Execution Delay**: 2 days before implementation

## Oracle Integration

### Quality Oracle

Provides real-time quality scores for carbon credit projects:

- **Verification scores** based on monitoring data
- **Project type assessment** comparing similar projects
- **Additionality verification** ensuring genuine impact
- **Permanence scoring** for long-term impact validation

### Volatility Oracle

Tracks market volatility for risk assessment:

- **Price volatility** indicating market confidence
- **Trading volume** showing market liquidity
- **Time-weighted averages** for stable assessments

## Security Features

### Access Control

- **Role-based permissions** for all critical functions
- **Multi-signature requirements** for admin functions
- **Time locks** on parameter updates

### Circuit Breakers

- **Emergency pause** capability for critical issues
- **Maximum payout limits** per claim and time period
- **Solvency monitoring** with automatic restrictions

### Audit & Validation

- **Oracle data validation** with multiple sources
- **Cross-reference checks** between oracle feeds
- **Historical data analysis** for anomaly detection

## Usage Examples

### Example 1: Basic Policy Creation

```javascript
// 1. User owns 5000 kg CO2e of renewable energy credits
const tokenId = 1;
const coveredAmount = 2000; // Insure 2000 kg CO2e
const coverageTypes = [0, 1]; // PROJECT_FAILURE, VERIFICATION_RISK
const duration = 90 * 24 * 60 * 60; // 90 days

// 2. Calculate premium
const premiumData = await insurance.calculatePremium(
  tokenId,
  coveredAmount,
  coverageTypes,
  duration
);

// 3. Approve premium payment
await wrfToken.approve(insurance.address, premiumData.finalPremium);

// 4. Create policy
const policyId = await insurance.createPolicy(
  tokenId,
  coveredAmount,
  coverageTypes,
  duration,
  'QmPolicyTermsHash...'
);

// 5. Pay premium
await insurance.payPremium(policyId);
```

### Example 2: Capital Provider Participation

```javascript
// 1. Deposit capital into insurance pool
const depositAmount = ethers.parseEther('50000'); // 50k WRF
await wrfToken.approve(insurancePool.address, depositAmount);
const shares = await insurancePool.depositCapital(depositAmount);

// 2. Earn rewards over time
// Rewards are automatically calculated based on pool performance

// 3. Withdraw capital (with 2-day delay)
await insurancePool.requestWithdrawal(shares);
// Wait 2 days...
await insurancePool.withdrawCapital(shares);
```

### Example 3: Claim Submission

```javascript
// 1. Project failure detected
const claimAmount = 1000; // 1000 kg CO2e affected
const claimType = 0; // PROJECT_FAILURE

// 2. Submit claim with evidence
const claimId = await insurance.submitClaim(
  policyId,
  claimType,
  claimAmount,
  'QmEvidenceHash...' // IPFS hash of supporting documents
);

// 3. System automatically processes if criteria met
// OR governance review for complex cases
```

## Integration Guide

### Frontend Integration

```javascript
// Connect to insurance contract
const insurance = new ethers.Contract(INSURANCE_ADDRESS, INSURANCE_ABI, provider);

// Get policy information
const policy = await insurance.getPolicy(policyId);

// Calculate premium for new policy
const premium = await insurance.calculatePremium(tokenId, amount, coverageTypes, duration);

// Monitor events
insurance.on('PolicyCreated', (policyId, holder, tokenId, amount, premium) => {
  console.log(`New policy ${policyId} created for ${amount} credits`);
});

insurance.on('ClaimSubmitted', (claimId, policyId, claimant, type, amount) => {
  console.log(`Claim ${claimId} submitted for policy ${policyId}`);
});
```

### API Integration

The protocol emits comprehensive events for external monitoring:

```solidity
event PolicyCreated(uint256 indexed policyId, address indexed policyholder, ...);
event ClaimSubmitted(uint256 indexed claimId, uint256 indexed policyId, ...);
event ClaimProcessed(uint256 indexed claimId, ClaimStatus status, uint256 payoutAmount);
event RiskAssessmentUpdated(uint256 indexed tokenId, uint256 newRiskScore, RiskLevel riskLevel);
```

## Configuration Parameters

### Premium Rates (Basis Points)

- Project Failure: 300 (3%)
- Verification Risk: 200 (2%)
- Regulatory Risk: 150 (1.5%)
- Delivery Guarantee: 100 (1%)
- Invalidation Cover: 250 (2.5%)

### Risk Multipliers

- Low Risk (0-25%): 0.5x
- Medium Risk (26-50%): 1.0x
- High Risk (51-75%): 2.0x
- Critical Risk (76-100%): 4.0x

### Pool Parameters

- Minimum Solvency Ratio: 120%
- Reward Rate: 5% annual
- Minimum Deposit: 1000 WRF
- Withdrawal Delay: 2 days

### Governance Parameters

- Voting Period: 7 days
- Execution Delay: 2 days
- Quorum Threshold: 40%
- Approval Threshold: 51%
- Minimum Proposal Stake: 10,000 WRF

## Testing

Run the comprehensive test suite:

```bash
# Run all insurance tests
npx hardhat test test/CarbonCreditInsurance.test.js

# Run specific test categories
npx hardhat test test/CarbonCreditInsurance.test.js --grep "Policy Creation"
npx hardhat test test/CarbonCreditInsurance.test.js --grep "Claims Processing"
npx hardhat test test/CarbonCreditInsurance.test.js --grep "Governance"
```

## Deployment

Deploy the complete insurance protocol:

```bash
# Deploy to local network
npx hardhat run scripts/deploy-insurance.js --network localhost

# Deploy to testnet
npx hardhat run scripts/deploy-insurance.js --network goerli

# Deploy to mainnet
npx hardhat run scripts/deploy-insurance.js --network mainnet
```

## Monitoring & Analytics

### Key Metrics to Track

1. **Pool Health**

   - Total capital vs. reserved capital
   - Solvency ratio
   - Number of active policies

2. **Claims Performance**

   - Claims frequency by coverage type
   - Average payout amounts
   - Processing time for claims

3. **Risk Assessment Accuracy**

   - Predicted vs. actual claim rates
   - Oracle data correlation with outcomes
   - Risk model performance

4. **Governance Activity**
   - Proposal creation frequency
   - Voting participation rates
   - Execution success rates

### Dashboard Queries

```javascript
// Get pool statistics
const poolStats = await insurancePool.getPoolStats();

// Get user's policies
const userPolicies = await insurance.getUserPolicies(userAddress);

// Get token's risk assessment
const riskData = await insurance.assessRisk(tokenId);

// Get active claims
const claim = await insurance.getClaim(claimId);
```

## Future Enhancements

### Planned Features

1. **Cross-Chain Insurance** - Extend coverage to carbon credits on other blockchains
2. **Parametric Insurance** - Automatic payouts based on objective triggers
3. **Insurance Marketplace** - Multiple insurance providers competing on rates
4. **Advanced Analytics** - AI-powered risk assessment models
5. **Integration APIs** - Easy integration for carbon credit marketplaces

### Research Areas

1. **Dynamic Pricing** - Real-time premium adjustments based on market conditions
2. **Catastrophic Risk Pooling** - Shared pools for large-scale environmental disasters
3. **Predictive Claims** - Early warning systems for potential claims
4. **Cross-Registry Standards** - Universal risk assessment across different registries

## Conclusion

The Carbon Credit Insurance Protocol provides comprehensive protection for carbon credit
investments, combining automated risk assessment with community governance. By leveraging oracle
data and decentralized capital pools, it offers a robust, transparent, and scalable solution for
managing carbon credit risks.

The protocol is designed to evolve with the carbon credit market, incorporating new risk factors and
coverage types as the ecosystem matures. Through its governance system, stakeholders can
continuously improve the protocol to better serve the needs of carbon credit investors and project
developers.
