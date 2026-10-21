# WRFCoin Smart Contract Templates - Security Guidelines & Audit Practices

## Security-First Design Principles

### 1. Defense in Depth

All templates implement multiple layers of security:

- **Access Control**: Role-based permissions with OpenZeppelin's AccessControl
- **Reentrancy Protection**: ReentrancyGuard on state-changing functions
- **Emergency Controls**: Pausable functionality and emergency roles
- **Input Validation**: Comprehensive parameter checking
- **Rate Limiting**: Built-in protection against abuse

### 2. Fail-Safe Defaults

- Contracts start in a safe state (often paused)
- Permissions are granted explicitly, not by default
- Operations require explicit authorization
- Default to the most restrictive settings

### 3. Principle of Least Privilege

- Roles have minimal necessary permissions
- Functions are restricted to appropriate roles
- Temporary permissions can be revoked
- Multi-signature requirements for critical operations

## Security Checklist for Template Usage

### Pre-Deployment Security Review

#### 1. Access Control Verification

```solidity
// ✅ GOOD: Proper role checks
function mint(address to, uint256 amount)
    external
    onlyRole(MINTER_ROLE)
    whenNotPaused
{
    _mint(to, amount);
}

// ❌ BAD: Missing access control
function mint(address to, uint256 amount) external {
    _mint(to, amount);
}
```

#### 2. Reentrancy Protection

```solidity
// ✅ GOOD: Protected against reentrancy
function purchaseCredits(uint256 listingId, uint256 amount)
    external
    payable
    nonReentrant
{
    // State changes before external calls
    listings[listingId].totalTons -= amount;

    // External calls last
    payable(seller).transfer(payment);
}

// ❌ BAD: Vulnerable to reentrancy
function purchaseCredits(uint256 listingId, uint256 amount)
    external
    payable
{
    payable(seller).transfer(payment);
    listings[listingId].totalTons -= amount; // State change after external call
}
```

#### 3. Input Validation

```solidity
// ✅ GOOD: Comprehensive validation
function setQualityScore(uint256 score) external {
    require(score <= 100, "Score must be <= 100");
    require(score > 0, "Score must be positive");
    require(hasRole(VERIFIER_ROLE, msg.sender), "Not authorized");

    qualityScore = score;
}

// ❌ BAD: Missing validation
function setQualityScore(uint256 score) external {
    qualityScore = score; // No bounds checking
}
```

#### 4. Safe Math and Overflow Protection

```solidity
// ✅ GOOD: Solidity 0.8+ has built-in overflow protection
function addFunds(uint256 amount) external {
    totalFunds += amount; // Safe in 0.8+
    require(totalFunds <= maxFunds, "Exceeds maximum");
}

// ✅ GOOD: Additional bounds checking
function calculateReward(uint256 stakeAmount, uint256 rate)
    external
    pure
    returns (uint256)
{
    require(rate <= 10000, "Rate too high"); // Max 100%
    return (stakeAmount * rate) / 10000;
}
```

### Template-Specific Security Considerations

#### Token Templates (BaseERC20Token, GovernanceToken)

**Critical Security Points:**

- Verify minting controls and supply limits
- Check for proper role management
- Ensure governance parameters are reasonable
- Validate voting power calculations

**Security Checklist:**

```solidity
// ✅ Verify these protections exist:
contract TokenSecurityChecklist {
    // 1. Supply controls
    uint256 public constant MAX_SUPPLY = 1000000000e18;

    function mint(address to, uint256 amount) external onlyRole(MINTER_ROLE) {
        require(totalSupply() + amount <= MAX_SUPPLY, "Exceeds max supply");
        _mint(to, amount);
    }

    // 2. Transfer controls
    function _beforeTokenTransfer(address from, address to, uint256 amount)
        internal
        override
        whenNotPaused
    {
        super._beforeTokenTransfer(from, to, amount);
    }

    // 3. Role management
    function grantRole(bytes32 role, address account)
        public
        override
        onlyRole(ADMIN_ROLE)
    {
        super.grantRole(role, account);
    }
}
```

#### NFT Templates (BaseERC721NFT)

**Critical Security Points:**

- Verify minting controls and supply limits
- Check royalty calculations
- Ensure metadata immutability where needed
- Validate reveal mechanisms

**Security Checklist:**

```solidity
contract NFTSecurityChecklist {
    // 1. Supply controls
    modifier withinSupplyLimit(uint256 quantity) {
        require(totalSupply() + quantity <= maxSupply, "Exceeds max supply");
        _;
    }

    // 2. Royalty protection
    function setRoyalty(uint256 basisPoints) external onlyRole(ADMIN_ROLE) {
        require(basisPoints <= 1000, "Royalty too high"); // Max 10%
        royaltyBasisPoints = basisPoints;
    }

    // 3. Metadata protection
    bool public metadataLocked;

    function lockMetadata() external onlyRole(ADMIN_ROLE) {
        metadataLocked = true;
    }

    function setBaseURI(string calldata newBaseURI)
        external
        onlyRole(ADMIN_ROLE)
    {
        require(!metadataLocked, "Metadata locked");
        baseURI = newBaseURI;
    }
}
```

#### Crowdfunding Templates

**Critical Security Points:**

- Verify time controls and caps
- Check refund mechanisms
- Ensure proper fund custody
- Validate vesting calculations

**Security Checklist:**

```solidity
contract CrowdfundingSecurityChecklist {
    // 1. Time controls
    modifier onlyWhileOpen() {
        require(block.timestamp >= openingTime, "Not yet open");
        require(block.timestamp <= closingTime, "Already closed");
        require(!hasClosed(), "Crowdsale closed");
        _;
    }

    // 2. Cap enforcement
    function _preValidatePurchase(address beneficiary, uint256 weiAmount)
        internal
        override
        view
    {
        super._preValidatePurchase(beneficiary, weiAmount);
        require(weiRaised() + weiAmount <= cap, "Cap exceeded");
    }

    // 3. Refund safety
    function claimRefund(address payable refundee)
        external
        nonReentrant
    {
        require(finalized, "Not finalized");
        require(!goalReached(), "Goal was reached");

        uint256 deposit = deposits[refundee];
        require(deposit > 0, "No deposit");

        deposits[refundee] = 0;
        refundee.transfer(deposit);
    }
}
```

#### Marketplace Templates

**Critical Security Points:**

- Verify listing authenticity
- Check payment calculations
- Ensure atomic transfers
- Validate fee calculations

**Security Checklist:**

```solidity
contract MarketplaceSecurityChecklist {
    // 1. Ownership verification
    function createListing(uint256[] calldata tokenIds, uint256 price)
        external
        nonReentrant
    {
        for (uint256 i = 0; i < tokenIds.length; i++) {
            require(
                IERC721(nftContract).ownerOf(tokenIds[i]) == msg.sender,
                "Not owner"
            );
        }
        // Create listing logic...
    }

    // 2. Atomic transfers
    function purchaseItem(uint256 listingId)
        external
        payable
        nonReentrant
    {
        Listing storage listing = listings[listingId];
        require(listing.isActive, "Not active");
        require(msg.value >= listing.price, "Insufficient payment");

        // Update state first
        listing.isActive = false;

        // Calculate fees
        uint256 fee = (listing.price * marketplaceFee) / 10000;
        uint256 sellerProceeds = listing.price - fee;

        // Transfer NFT
        IERC721(nftContract).safeTransferFrom(
            listing.seller,
            msg.sender,
            listing.tokenId
        );

        // Transfer payments
        payable(listing.seller).transfer(sellerProceeds);
        payable(feeRecipient).transfer(fee);

        // Refund excess
        if (msg.value > listing.price) {
            payable(msg.sender).transfer(msg.value - listing.price);
        }
    }
}
```

#### Governance Templates

**Critical Security Points:**

- Verify voting mechanisms
- Check timelock delays
- Ensure proposal validation
- Validate quorum calculations

**Security Checklist:**

```solidity
contract GovernanceSecurityChecklist {
    // 1. Proposal validation
    function propose(
        address[] memory targets,
        uint256[] memory values,
        bytes[] memory calldatas,
        string memory description
    ) public override returns (uint256) {
        require(
            getVotes(msg.sender, block.number - 1) >= proposalThreshold(),
            "Insufficient voting power"
        );
        require(targets.length == values.length, "Array length mismatch");
        require(targets.length == calldatas.length, "Array length mismatch");
        require(targets.length > 0, "Empty proposal");

        return super.propose(targets, values, calldatas, description);
    }

    // 2. Execution safety
    function execute(
        address[] memory targets,
        uint256[] memory values,
        bytes[] memory calldatas,
        bytes32 descriptionHash
    ) public payable override returns (uint256) {
        uint256 proposalId = super.execute(targets, values, calldatas, descriptionHash);

        // Additional validation for high-value proposals
        uint256 totalValue = 0;
        for (uint256 i = 0; i < values.length; i++) {
            totalValue += values[i];
        }

        if (totalValue > emergencyThreshold) {
            require(
                hasRole(EMERGENCY_APPROVER_ROLE, msg.sender),
                "Requires emergency approval"
            );
        }

        return proposalId;
    }
}
```

## Audit Guidelines

### 1. Pre-Audit Preparation

**Code Documentation:**

- Ensure all functions have comprehensive NatSpec comments
- Document business logic and edge cases
- Provide deployment and configuration instructions
- Include test coverage reports

**Test Coverage:**

- Aim for >95% line coverage
- Include edge case testing
- Test all access control mechanisms
- Verify emergency scenarios

### 2. Audit Focus Areas

#### High Priority

1. **Access Control**: Role assignments and permissions
2. **Fund Safety**: Payment flows and fund custody
3. **External Interactions**: Oracle calls and external contracts
4. **Arithmetic Operations**: Calculations and overflows
5. **State Management**: Critical state transitions

#### Medium Priority

1. **Gas Optimization**: Efficient operations
2. **User Experience**: Clear error messages
3. **Upgradeability**: Proxy patterns if used
4. **Documentation**: Code clarity and comments

#### Low Priority

1. **Code Style**: Formatting and naming
2. **Optimization**: Minor gas savings
3. **Documentation**: Additional examples

### 3. Common Vulnerabilities to Check

#### Reentrancy

```solidity
// ✅ GOOD: Checks-Effects-Interactions pattern
function withdraw(uint256 amount) external nonReentrant {
    require(balances[msg.sender] >= amount, "Insufficient balance");

    // Effects: Update state first
    balances[msg.sender] -= amount;

    // Interactions: External calls last
    payable(msg.sender).transfer(amount);
}
```

#### Front-running

```solidity
// ✅ GOOD: Commit-reveal scheme for sensitive operations
mapping(bytes32 => bool) public commitments;
mapping(address => uint256) public reveals;

function commitBid(bytes32 commitment) external {
    commitments[commitment] = true;
}

function revealBid(uint256 amount, uint256 nonce) external {
    bytes32 commitment = keccak256(abi.encodePacked(msg.sender, amount, nonce));
    require(commitments[commitment], "Invalid commitment");

    reveals[msg.sender] = amount;
    delete commitments[commitment];
}
```

#### Oracle Manipulation

```solidity
// ✅ GOOD: Multiple oracle sources and validation
function getPrice() external view returns (uint256) {
    uint256 price1 = oracle1.getPrice();
    uint256 price2 = oracle2.getPrice();
    uint256 price3 = oracle3.getPrice();

    // Use median price to avoid manipulation
    uint256[] memory prices = new uint256[](3);
    prices[0] = price1;
    prices[1] = price2;
    prices[2] = price3;

    return _median(prices);
}
```

#### Timestamp Dependence

```solidity
// ✅ GOOD: Use block.number for relative timing
uint256 public constant VOTING_DURATION = 46523; // ~7 days in blocks

function vote(uint256 proposalId) external {
    Proposal storage proposal = proposals[proposalId];
    require(
        block.number >= proposal.startBlock &&
        block.number <= proposal.startBlock + VOTING_DURATION,
        "Voting period invalid"
    );
}
```

### 4. Emergency Response Procedures

#### Emergency Roles and Functions

```solidity
contract EmergencyControls {
    bytes32 public constant EMERGENCY_ROLE = keccak256("EMERGENCY_ROLE");

    bool public emergencyStop;

    modifier notInEmergency() {
        require(!emergencyStop, "Emergency stop active");
        _;
    }

    function emergencyPause() external onlyRole(EMERGENCY_ROLE) {
        emergencyStop = true;
        _pause();
    }

    function emergencyUnpause() external onlyRole(ADMIN_ROLE) {
        emergencyStop = false;
        _unpause();
    }

    function emergencyWithdraw(address token, uint256 amount)
        external
        onlyRole(EMERGENCY_ROLE)
    {
        require(emergencyStop, "Must be in emergency");

        if (token == address(0)) {
            payable(msg.sender).transfer(amount);
        } else {
            IERC20(token).transfer(msg.sender, amount);
        }
    }
}
```

#### Multi-signature Requirements

```solidity
contract MultiSigEmergency {
    mapping(bytes32 => uint256) public emergencyApprovals;
    mapping(bytes32 => mapping(address => bool)) public hasApproved;

    uint256 public constant REQUIRED_APPROVALS = 3;

    function approveEmergencyAction(bytes32 actionHash)
        external
        onlyRole(EMERGENCY_ROLE)
    {
        require(!hasApproved[actionHash][msg.sender], "Already approved");

        hasApproved[actionHash][msg.sender] = true;
        emergencyApprovals[actionHash]++;
    }

    function executeEmergencyAction(
        address target,
        bytes calldata data
    ) external onlyRole(EMERGENCY_ROLE) {
        bytes32 actionHash = keccak256(abi.encodePacked(target, data));
        require(
            emergencyApprovals[actionHash] >= REQUIRED_APPROVALS,
            "Insufficient approvals"
        );

        (bool success,) = target.call(data);
        require(success, "Emergency action failed");

        // Reset approvals
        emergencyApprovals[actionHash] = 0;
    }
}
```

## Deployment Security

### 1. Deployment Checklist

- [ ] All constructor parameters verified
- [ ] Initial roles assigned correctly
- [ ] Emergency controls configured
- [ ] Pausable state set appropriately
- [ ] Oracle addresses validated
- [ ] Fee parameters within acceptable ranges
- [ ] Time parameters reasonable
- [ ] Supply limits configured correctly

### 2. Post-Deployment Verification

- [ ] Verify contract source code on block explorer
- [ ] Test all major functions with small amounts
- [ ] Verify role assignments through read functions
- [ ] Check emergency pause/unpause functionality
- [ ] Validate oracle data feeds
- [ ] Monitor for unexpected events or errors

### 3. Ongoing Security Monitoring

- [ ] Set up event monitoring and alerts
- [ ] Regular balance and state checks
- [ ] Monitor for unusual transaction patterns
- [ ] Keep track of role changes
- [ ] Regular security reviews as code evolves

## Security Tools and Resources

### Recommended Tools

1. **Slither**: Static analysis tool
2. **Mythril**: Security analysis platform
3. **Echidna**: Fuzzing framework
4. **Manticore**: Symbolic execution tool
5. **OpenZeppelin Defender**: Monitoring and alerting

### Testing Frameworks

1. **Hardhat**: Development environment
2. **Foundry**: Fast testing framework
3. **Brownie**: Python-based framework

### Audit Services

1. **Trail of Bits**
2. **OpenZeppelin**
3. **ConsenSys Diligence**
4. **Sigma Prime**
5. **CertiK**

Remember: Security is an ongoing process, not a one-time check. Regular reviews, monitoring, and
updates are essential for maintaining the security of your smart contracts.
