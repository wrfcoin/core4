# WRFCoin Smart Contract Templates - Examples & Usage Guide

## Quick Start Examples

### 1. Basic ERC20 Token

```solidity
// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import "./tokens/BaseERC20Token.sol";

contract MyToken is BaseERC20Token {
    constructor(address initialOwner)
        BaseERC20Token(
            "My Token",
            "MTK",
            18,
            1000000 * 10**18, // 1M tokens
            initialOwner
        )
    {
        // Additional initialization if needed
    }
}
```

**Deployment:**

```javascript
const MyToken = await ethers.getContractFactory('MyToken');
const token = await MyToken.deploy(deployer.address);
```

### 2. Governance Token with Voting

```solidity
// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import "./tokens/GovernanceToken.sol";

contract MyDAOToken is GovernanceToken {
    constructor(address initialOwner)
        GovernanceToken(
            "MyDAO Token",
            "MYDAO",
            18,
            10000000 * 10**18, // 10M tokens
            initialOwner
        )
    {
        // Set up governance parameters
        _setupGovernanceParameters(
            7 days,    // Voting period
            100000,    // Proposal threshold (tokens needed to propose)
            500        // Quorum (5%)
        );
    }
}
```

### 3. Climate-Focused NFT Collection

```solidity
// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import "./nft/BaseERC721NFT.sol";

contract CarbonCreditNFT is BaseERC721NFT {
    struct CarbonData {
        uint256 co2Offset;
        string projectType;
        string location;
        uint256 vintageYear;
    }

    mapping(uint256 => CarbonData) public carbonData;

    constructor(address initialOwner)
        BaseERC721NFT(
            "Carbon Credit NFT",
            "CARBON",
            1000,  // Max supply
            0.1 ether,  // Mint price
            100,   // Royalty basis points (1%)
            initialOwner
        )
    {}

    function mintCarbonCredit(
        address to,
        uint256 co2Offset,
        string calldata projectType,
        string calldata location,
        uint256 vintageYear,
        string calldata tokenURI
    ) external onlyRole(MINTER_ROLE) {
        uint256 tokenId = _nextTokenId();

        carbonData[tokenId] = CarbonData({
            co2Offset: co2Offset,
            projectType: projectType,
            location: location,
            vintageYear: vintageYear
        });

        _safeMint(to, tokenId);
        _setTokenURI(tokenId, tokenURI);
    }
}
```

### 4. Carbon Credit Marketplace

```solidity
// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import "./climate/CarbonMarketplace.sol";

contract MyMarketplace is CarbonMarketplace {
    constructor(address initialOwner) CarbonMarketplace(initialOwner) {
        // Marketplace-specific initialization
        _setupMarketplaceFees(250); // 2.5% marketplace fee
    }

    // Example: Create verified project credits
    function createProjectCredits(
        string calldata projectName,
        string calldata location,
        uint256 totalCredits,
        QualityMetrics calldata quality
    ) external onlyVerifiedIssuer {
        for (uint256 i = 0; i < totalCredits; i++) {
            issueCredits(
                CreditType.FOREST_CONSERVATION,
                CreditStandard.VCS,
                1, // 1 ton per credit
                projectName,
                location,
                "ipfs://verification-doc-hash",
                2024, // Vintage year
                block.timestamp + 365 days, // 1 year validity
                quality
            );
        }
    }
}
```

### 5. Crowdfunding Campaign

```solidity
// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import "./crowdfunding/VestingCrowdsale.sol";

contract ClimateProjectFunding is VestingCrowdsale {
    constructor(
        address token,
        address wallet,
        uint256 rate
    )
        VestingCrowdsale(
            token,
            wallet,
            rate,
            1000 ether,    // Soft cap
            10000 ether,   // Hard cap
            block.timestamp + 7 days,   // Start time
            block.timestamp + 30 days   // End time
        )
    {
        // Set up vesting for early investors
        _setupDefaultVesting(
            VestingType.LINEAR,
            365 days,  // Vesting duration
            90 days,   // Cliff period
            10         // Release steps
        );
    }

    // Custom bonus structure
    function _getTokenAmount(uint256 weiAmount)
        internal
        view
        override
        returns (uint256)
    {
        uint256 tokens = super._getTokenAmount(weiAmount);

        // Early bird bonus (first week)
        if (block.timestamp <= openingTime + 7 days) {
            tokens = tokens * 120 / 100; // 20% bonus
        }
        // Regular bonus (second week)
        else if (block.timestamp <= openingTime + 14 days) {
            tokens = tokens * 110 / 100; // 10% bonus
        }

        return tokens;
    }
}
```

### 6. DAO with Treasury Management

```solidity
// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import "./governance/AdvancedGovernor.sol";
import "./governance/TreasuryManager.sol";
import "./governance/TimelockController.sol";

contract ClimateDAO {
    AdvancedGovernor public governor;
    TreasuryManager public treasury;
    TimelockController public timelock;

    constructor(address governanceToken) {
        // Deploy timelock first
        timelock = new TimelockController(
            2 days,    // Min delay
            new address[](0), // Proposers (will be set to governor)
            new address[](0), // Executors (will be set to governor)
            msg.sender // Admin
        );

        // Deploy treasury with timelock
        treasury = new TreasuryManager(msg.sender, address(timelock));

        // Deploy governor
        governor = new AdvancedGovernor(
            governanceToken,
            address(timelock),
            "Climate DAO",
            7 days,     // Voting period
            1 days,     // Voting delay
            100000,     // Proposal threshold
            500         // Quorum (5%)
        );

        // Set up roles
        timelock.grantRole(timelock.PROPOSER_ROLE(), address(governor));
        timelock.grantRole(timelock.EXECUTOR_ROLE(), address(governor));

        // Revoke admin role from deployer
        timelock.revokeRole(timelock.TIMELOCK_ADMIN_ROLE(), msg.sender);
    }
}
```

### 7. Weather Data Oracle Integration

```solidity
// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import "./oracles/WeatherDataOracle.sol";

contract CropInsurance {
    WeatherDataOracle public weatherOracle;

    struct Policy {
        address farmer;
        int256 latitude;
        int256 longitude;
        uint256 coverageAmount;
        uint256 premiumPaid;
        uint256 temperatureThreshold; // If exceeded, payout
        bool isActive;
        bool isPaidOut;
    }

    mapping(uint256 => Policy) public policies;
    uint256 public policyCount;

    constructor(address _weatherOracle) {
        weatherOracle = WeatherDataOracle(_weatherOracle);
    }

    function purchasePolicy(
        int256 latitude,
        int256 longitude,
        uint256 coverageAmount,
        uint256 temperatureThreshold
    ) external payable {
        require(msg.value >= coverageAmount / 10, "Insufficient premium");

        policies[policyCount] = Policy({
            farmer: msg.sender,
            latitude: latitude,
            longitude: longitude,
            coverageAmount: coverageAmount,
            premiumPaid: msg.value,
            temperatureThreshold: temperatureThreshold,
            isActive: true,
            isPaidOut: false
        });

        policyCount++;
    }

    function claimPayout(uint256 policyId) external {
        Policy storage policy = policies[policyId];
        require(policy.farmer == msg.sender, "Not policy holder");
        require(policy.isActive && !policy.isPaidOut, "Policy not active");

        // Get weather data from oracle
        (
            int256[] memory temperatures,
            , , , ,
            uint256 quality
        ) = weatherOracle.getCurrentWeather(
            policy.latitude,
            policy.longitude,
            5000 // 5km radius
        );

        // Check if conditions met
        bool payoutTriggered = false;
        for (uint256 i = 0; i < temperatures.length; i++) {
            if (temperatures[i] > int256(policy.temperatureThreshold)) {
                payoutTriggered = true;
                break;
            }
        }

        if (payoutTriggered && quality >= 70) {
            policy.isPaidOut = true;
            payable(policy.farmer).transfer(policy.coverageAmount);
        }
    }
}
```

## Integration Patterns

### 1. Multi-Template Integration

```solidity
contract ClimateEcosystem {
    // Combine multiple templates
    GovernanceToken public governanceToken;
    CarbonMarketplace public marketplace;
    WeatherDataOracle public oracle;
    AdvancedGovernor public governor;

    function setupEcosystem() external {
        // Deploy and connect all components
        governanceToken = new GovernanceToken(/* params */);
        marketplace = new CarbonMarketplace(address(this));
        oracle = new WeatherDataOracle(address(this));

        // Set up cross-contract permissions
        marketplace.grantRole(marketplace.VERIFIER_ROLE(), address(oracle));
        oracle.grantRole(oracle.OPERATOR_ROLE(), address(marketplace));
    }
}
```

### 2. Upgradeable Template Pattern

```solidity
import "@openzeppelin/contracts-upgradeable/proxy/utils/Initializable.sol";

contract UpgradeableToken is Initializable, BaseERC20Token {
    function initialize(
        string memory name,
        string memory symbol,
        address owner
    ) public initializer {
        __BaseERC20Token_init(name, symbol, 18, 1000000e18, owner);
    }
}
```

### 3. Template Composition

```solidity
contract CustomCrowdsale is
    VestingCrowdsale,
    WhitelistCrowdsale,
    RefundableCrowdsale
{
    // Combine multiple crowdsale features
    constructor(/* params */)
        VestingCrowdsale(/* params */)
        WhitelistCrowdsale(/* params */)
        RefundableCrowdsale(/* params */)
    {}
}
```

## Testing Examples

### 1. Basic Token Test

```javascript
const { expect } = require('chai');
const { ethers } = require('hardhat');

describe('MyToken', function () {
  let token, owner, addr1, addr2;

  beforeEach(async function () {
    [owner, addr1, addr2] = await ethers.getSigners();
    const Token = await ethers.getContractFactory('MyToken');
    token = await Token.deploy(owner.address);
  });

  it('Should have correct initial supply', async function () {
    expect(await token.totalSupply()).to.equal(ethers.parseEther('1000000'));
  });

  it('Should allow minting by admin', async function () {
    await token.mint(addr1.address, ethers.parseEther('1000'));
    expect(await token.balanceOf(addr1.address)).to.equal(ethers.parseEther('1000'));
  });
});
```

### 2. Marketplace Test

```javascript
describe('CarbonMarketplace', function () {
  let marketplace, issuer, buyer;

  beforeEach(async function () {
    [owner, issuer, buyer] = await ethers.getSigners();
    const Marketplace = await ethers.getContractFactory('CarbonMarketplace');
    marketplace = await Marketplace.deploy(owner.address);

    // Verify issuer
    await marketplace.verifyIssuer(issuer.address, true);
  });

  it('Should allow issuing credits', async function () {
    const quality = {
      additionality: 90,
      permanence: 85,
      measurability: 95,
      verification: 90,
      cobenefits: 80,
      transparency: 90,
    };

    await marketplace.connect(issuer).issueCredits(
      0, // FOREST_CONSERVATION
      0, // VCS
      100, // 100 tons
      'Test Project',
      'Amazon',
      'ipfs://doc',
      2024,
      Date.now() + 365 * 24 * 3600, // 1 year
      quality
    );

    const credit = await marketplace.getCreditDetails(0);
    expect(credit.carbonOffsetTons).to.equal(100);
  });
});
```

## Common Use Cases

### 1. Environmental Project Funding

- Use VestingCrowdsale for project funding
- Integrate with EnvironmentalImpact for milestone tracking
- Use CarbonMarketplace for credit trading

### 2. Corporate Sustainability Programs

- Deploy GovernanceToken for stakeholder voting
- Use CarbonFootprintTracker for emissions tracking
- Integrate TreasuryManager for sustainability fund management

### 3. Climate Data Monetization

- Use WeatherDataOracle for data provision
- Integrate with BaseERC20Token for payment tokens
- Use BasicDAO for governance of data standards

### 4. Carbon Offset Platform

- Combine CarbonMarketplace with BaseERC721NFT
- Use EnvironmentalImpact for project verification
- Integrate GovernanceToken for platform governance

## Next Steps

1. **Choose Your Template**: Select the appropriate template based on your use case
2. **Customize**: Modify the template to fit your specific requirements
3. **Test**: Use the provided test examples as a starting point
4. **Deploy**: Follow the deployment patterns for your network
5. **Integrate**: Connect multiple templates for comprehensive solutions

For security considerations and audit guidelines, see `SECURITY.md`.
