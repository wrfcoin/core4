// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import "../base/SecurityBase.sol";
import "./TimelockController.sol";

/**
 * @title TreasuryManager
 * @dev Comprehensive treasury management system for DAOs
 * 
 * Features:
 * - Multi-asset treasury management
 * - Spending proposals and limits
 * - Yield generation strategies
 * - Budget allocation and tracking
 * - Performance reporting
 * - Risk management controls
 * - Diversification enforcement
 * 
 * Use Cases:
 * - DAO treasury management
 * - Protocol fee management
 * - Grant distribution
 * - Yield farming strategies
 * - Investment diversification
 */
contract TreasuryManager is SecurityBase {
    enum AssetType {
        NATIVE,     // ETH or native token
        ERC20,      // ERC20 tokens
        ERC721,     // NFTs
        LP_TOKEN,   // Liquidity pool tokens
        YIELD_BEARING // Yield bearing assets (staking, lending)
    }
    
    enum ProposalType {
        SPENDING,           // Direct spending proposal
        INVESTMENT,         // Investment in yield strategies
        BUDGET_ALLOCATION,  // Budget allocation changes
        ASSET_MANAGEMENT,   // Asset management operations
        EMERGENCY_FUND      // Emergency fund access
    }
    
    enum ProposalStatus {
        PENDING,    // Proposal created
        APPROVED,   // Proposal approved
        EXECUTED,   // Proposal executed
        REJECTED,   // Proposal rejected
        EXPIRED     // Proposal expired
    }
    
    struct Asset {
        address tokenAddress;
        AssetType assetType;
        uint256 balance;
        uint256 targetAllocation; // Percentage (basis points)
        uint256 currentAllocation; // Percentage (basis points)
        uint256 riskScore;        // 1-100, 100 being highest risk
        bool isActive;
        uint256 lastUpdated;
    }
    
    struct TreasuryProposal {
        uint256 id;
        ProposalType proposalType;
        address proposer;
        string title;
        string description;
        address targetAsset;
        uint256 amount;
        address recipient;
        uint256 deadline;
        ProposalStatus status;
        uint256 forVotes;
        uint256 againstVotes;
        uint256 createdAt;
        bytes executionData;
        bool executed;
    }
    
    struct Budget {
        string category;
        uint256 allocated;
        uint256 spent;
        uint256 period;        // Budget period in seconds
        uint256 periodStart;
        bool isActive;
    }
    
    struct YieldStrategy {
        address strategyContract;
        address targetAsset;
        uint256 allocatedAmount;
        uint256 currentValue;
        uint256 expectedYield;  // Annual percentage (basis points)
        uint256 riskScore;      // 1-100
        bool isActive;
        uint256 lastHarvest;
    }
    
    struct DiversificationRule {
        uint256 maxSingleAssetPercentage;    // Max % for single asset
        uint256 maxHighRiskPercentage;       // Max % for high risk assets
        uint256 minStablePercentage;         // Min % for stable assets
        uint256 maxYieldStrategyPercentage;  // Max % in yield strategies
    }
    
    // Treasury state
    mapping(address => Asset) public assets;
    address[] public assetList;
    uint256 public totalTreasuryValue;
    
    // Proposals
    mapping(uint256 => TreasuryProposal) public proposals;
    mapping(uint256 => mapping(address => bool)) public hasVoted;
    uint256 public proposalCount;
    
    // Budgets
    mapping(string => Budget) public budgets;
    string[] public budgetCategories;
    
    // Yield strategies
    mapping(address => YieldStrategy) public yieldStrategies;
    address[] public activeStrategies;
    
    // Diversification and risk management
    DiversificationRule public diversificationRule;
    uint256 public emergencyFundPercentage = 1000; // 10%
    uint256 public maxProposalAmount = 1000; // 10% of treasury per proposal
    
    // Access control
    bytes32 public constant TREASURER_ROLE = keccak256("TREASURER_ROLE");
    bytes32 public constant BUDGET_MANAGER_ROLE = keccak256("BUDGET_MANAGER_ROLE");
    bytes32 public constant STRATEGY_MANAGER_ROLE = keccak256("STRATEGY_MANAGER_ROLE");
    
    // Timelock integration
    TimelockController public timelock;
    
    // Voting parameters
    uint256 public proposalThreshold = 100 * 10**18; // 100 tokens to propose
    uint256 public votingPeriod = 7 days;
    uint256 public quorum = 500; // 5%
    
    // Performance tracking
    mapping(uint256 => uint256) public monthlyReturns; // month => return in basis points
    uint256 public totalFeesEarned;
    uint256 public totalYieldGenerated;
    
    // Events
    event AssetAdded(address indexed asset, AssetType assetType, uint256 targetAllocation);
    event AssetUpdated(address indexed asset, uint256 newBalance, uint256 newAllocation);
    event ProposalCreated(
        uint256 indexed proposalId,
        ProposalType proposalType,
        address indexed proposer,
        uint256 amount
    );
    event ProposalExecuted(uint256 indexed proposalId, bool success);
    event BudgetCreated(string category, uint256 allocated, uint256 period);
    event BudgetSpent(string category, uint256 amount, address recipient);
    event YieldStrategyAdded(address indexed strategy, address asset, uint256 allocation);
    event YieldHarvested(address indexed strategy, uint256 amount);
    event DiversificationRuleUpdated();
    event EmergencyWithdrawal(address indexed asset, uint256 amount, address recipient);
    
    // Errors
    error InsufficientBalance();
    error ExceedsMaxProposalAmount();
    error ViolatesDiversificationRules();
    error BudgetExceeded();
    error ProposalNotActive();
    error AlreadyVoted();
    error NotAuthorizedForEmergency();
    error InvalidRiskScore();
    error StrategyNotActive();
    
    modifier onlyTreasurer() {
        require(hasRole(TREASURER_ROLE, msg.sender), "TreasuryManager: Not treasurer");
        _;
    }
    
    modifier onlyBudgetManager() {
        require(hasRole(BUDGET_MANAGER_ROLE, msg.sender), "TreasuryManager: Not budget manager");
        _;
    }
    
    modifier onlyStrategyManager() {
        require(hasRole(STRATEGY_MANAGER_ROLE, msg.sender), "TreasuryManager: Not strategy manager");
        _;
    }
    
    constructor(
        address initialOwner,
        address _timelock
    ) SecurityBase(initialOwner) {
        timelock = TimelockController(_timelock);
        
        // Set up initial roles
        _grantRole(TREASURER_ROLE, initialOwner);
        _grantRole(BUDGET_MANAGER_ROLE, initialOwner);
        _grantRole(STRATEGY_MANAGER_ROLE, initialOwner);
        
        // Set up diversification rules
        _setupDiversificationRules();
    }
    
    /**
     * @dev Receive ETH deposits
     */
    receive() external payable {
        _updateAssetBalance(address(0), address(this).balance);
        _updateTotalTreasuryValue();
    }
    
    /**
     * @dev Add asset to treasury management
     */
    function addAsset(
        address tokenAddress,
        AssetType assetType,
        uint256 targetAllocation,
        uint256 riskScore
    ) external onlyTreasurer {
        require(riskScore <= 100, "TreasuryManager: Invalid risk score");
        require(targetAllocation <= 10000, "TreasuryManager: Invalid allocation");
        
        assets[tokenAddress] = Asset({
            tokenAddress: tokenAddress,
            assetType: assetType,
            balance: 0,
            targetAllocation: targetAllocation,
            currentAllocation: 0,
            riskScore: riskScore,
            isActive: true,
            lastUpdated: block.timestamp
        });
        
        assetList.push(tokenAddress);
        
        emit AssetAdded(tokenAddress, assetType, targetAllocation);
    }
    
    /**
     * @dev Create spending proposal
     */
    function createSpendingProposal(
        string calldata title,
        string calldata description,
        address targetAsset,
        uint256 amount,
        address recipient,
        string calldata budgetCategory
    ) external returns (uint256) {
        // Check proposal threshold (simplified - should check governance token balance)
        // require(governanceToken.balanceOf(msg.sender) >= proposalThreshold, "Insufficient tokens");
        
        // Check amount limits
        uint256 assetBalance = _getAssetBalance(targetAsset);
        require(amount <= (assetBalance * maxProposalAmount) / 10000, "TreasuryManager: Exceeds max proposal amount");
        
        // Check budget if specified
        if (bytes(budgetCategory).length > 0) {
            Budget storage budget = budgets[budgetCategory];
            require(budget.isActive, "TreasuryManager: Budget not active");
            require(budget.spent + amount <= budget.allocated, "TreasuryManager: Exceeds budget");
        }
        
        uint256 proposalId = proposalCount++;
        
        proposals[proposalId] = TreasuryProposal({
            id: proposalId,
            proposalType: ProposalType.SPENDING,
            proposer: msg.sender,
            title: title,
            description: description,
            targetAsset: targetAsset,
            amount: amount,
            recipient: recipient,
            deadline: block.timestamp + votingPeriod,
            status: ProposalStatus.PENDING,
            forVotes: 0,
            againstVotes: 0,
            createdAt: block.timestamp,
            executionData: "",
            executed: false
        });
        
        emit ProposalCreated(proposalId, ProposalType.SPENDING, msg.sender, amount);
        return proposalId;
    }
    
    /**
     * @dev Vote on treasury proposal
     */
    function vote(uint256 proposalId, bool support) external {
        TreasuryProposal storage proposal = proposals[proposalId];
        
        require(proposal.status == ProposalStatus.PENDING, "TreasuryManager: Proposal not active");
        require(block.timestamp <= proposal.deadline, "TreasuryManager: Voting period ended");
        require(!hasVoted[proposalId][msg.sender], "TreasuryManager: Already voted");
        
        hasVoted[proposalId][msg.sender] = true;
        
        // Simplified voting power (should integrate with governance token)
        uint256 votingPower = 1;
        
        if (support) {
            proposal.forVotes += votingPower;
        } else {
            proposal.againstVotes += votingPower;
        }
    }
    
    /**
     * @dev Execute approved proposal
     */
    function executeProposal(uint256 proposalId) external nonReentrant {
        TreasuryProposal storage proposal = proposals[proposalId];
        
        require(proposal.status == ProposalStatus.PENDING, "TreasuryManager: Proposal not pending");
        require(block.timestamp > proposal.deadline, "TreasuryManager: Voting still active");
        require(!proposal.executed, "TreasuryManager: Already executed");
        
        // Check if proposal passed
        uint256 totalVotes = proposal.forVotes + proposal.againstVotes;
        require(totalVotes >= quorum, "TreasuryManager: Quorum not reached");
        require(proposal.forVotes > proposal.againstVotes, "TreasuryManager: Proposal rejected");
        
        proposal.status = ProposalStatus.APPROVED;
        proposal.executed = true;
        
        // Execute the proposal
        bool success = _executeSpending(proposal.targetAsset, proposal.amount, proposal.recipient);
        
        if (success) {
            proposal.status = ProposalStatus.EXECUTED;
        }
        
        emit ProposalExecuted(proposalId, success);
    }
    
    /**
     * @dev Create budget allocation
     */
    function createBudget(
        string calldata category,
        uint256 allocated,
        uint256 period
    ) external onlyBudgetManager {
        budgets[category] = Budget({
            category: category,
            allocated: allocated,
            spent: 0,
            period: period,
            periodStart: block.timestamp,
            isActive: true
        });
        
        budgetCategories.push(category);
        
        emit BudgetCreated(category, allocated, period);
    }
    
    /**
     * @dev Spend from budget (direct spending for approved categories)
     */
    function spendFromBudget(
        string calldata category,
        uint256 amount,
        address recipient,
        address asset
    ) external onlyTreasurer {
        Budget storage budget = budgets[category];
        
        require(budget.isActive, "TreasuryManager: Budget not active");
        require(budget.spent + amount <= budget.allocated, "TreasuryManager: Budget exceeded");
        
        budget.spent += amount;
        
        _executeSpending(asset, amount, recipient);
        
        emit BudgetSpent(category, amount, recipient);
    }
    
    /**
     * @dev Add yield strategy
     */
    function addYieldStrategy(
        address strategyContract,
        address targetAsset,
        uint256 allocatedAmount,
        uint256 expectedYield,
        uint256 riskScore
    ) external onlyStrategyManager {
        require(riskScore <= 100, "TreasuryManager: Invalid risk score");
        
        // Check diversification rules
        _validateDiversification(targetAsset, allocatedAmount);
        
        yieldStrategies[strategyContract] = YieldStrategy({
            strategyContract: strategyContract,
            targetAsset: targetAsset,
            allocatedAmount: allocatedAmount,
            currentValue: allocatedAmount,
            expectedYield: expectedYield,
            riskScore: riskScore,
            isActive: true,
            lastHarvest: block.timestamp
        });
        
        activeStrategies.push(strategyContract);
        
        // Transfer assets to strategy
        _executeSpending(targetAsset, allocatedAmount, strategyContract);
        
        emit YieldStrategyAdded(strategyContract, targetAsset, allocatedAmount);
    }
    
    /**
     * @dev Harvest yield from strategy
     */
    function harvestYield(address strategyContract) external onlyStrategyManager {
        YieldStrategy storage strategy = yieldStrategies[strategyContract];
        require(strategy.isActive, "TreasuryManager: Strategy not active");
        
        // Call strategy harvest function (simplified)
        // uint256 harvestedAmount = IYieldStrategy(strategyContract).harvest();
        uint256 harvestedAmount = 0; // Placeholder
        
        strategy.lastHarvest = block.timestamp;
        totalYieldGenerated += harvestedAmount;
        
        emit YieldHarvested(strategyContract, harvestedAmount);
    }
    
    /**
     * @dev Emergency withdrawal (requires multiple signatures or governance)
     */
    function emergencyWithdraw(
        address asset,
        uint256 amount,
        address recipient
    ) external onlyRole(EMERGENCY_ROLE) {
        require(amount <= _getAssetBalance(asset), "TreasuryManager: Insufficient balance");
        
        _executeSpending(asset, amount, recipient);
        
        emit EmergencyWithdrawal(asset, amount, recipient);
    }
    
    /**
     * @dev Update diversification rules
     */
    function updateDiversificationRules(
        uint256 maxSingleAsset,
        uint256 maxHighRisk,
        uint256 minStable,
        uint256 maxYieldStrategy
    ) external onlyRole(ADMIN_ROLE) {
        diversificationRule = DiversificationRule({
            maxSingleAssetPercentage: maxSingleAsset,
            maxHighRiskPercentage: maxHighRisk,
            minStablePercentage: minStable,
            maxYieldStrategyPercentage: maxYieldStrategy
        });
        
        emit DiversificationRuleUpdated();
    }
    
    /**
     * @dev Update asset balances and allocations
     */
    function updateAssetBalances() external {
        uint256 totalValue = 0;
        
        for (uint256 i = 0; i < assetList.length; i++) {
            address asset = assetList[i];
            uint256 balance = _getAssetBalance(asset);
            
            assets[asset].balance = balance;
            assets[asset].lastUpdated = block.timestamp;
            
            totalValue += balance;
        }
        
        // Update current allocations
        for (uint256 i = 0; i < assetList.length; i++) {
            address asset = assetList[i];
            if (totalValue > 0) {
                assets[asset].currentAllocation = (assets[asset].balance * 10000) / totalValue;
            }
        }
        
        totalTreasuryValue = totalValue;
    }
    
    /**
     * @dev Get treasury performance report
     */
    function getTreasuryReport() external view returns (
        uint256 totalValue,
        uint256 totalYield,
        uint256 totalFees,
        uint256 activeStrategiesCount,
        uint256 activeBudgetsCount
    ) {
        return (
            totalTreasuryValue,
            totalYieldGenerated,
            totalFeesEarned,
            activeStrategies.length,
            budgetCategories.length
        );
    }
    
    /**
     * @dev Get asset allocation report
     */
    function getAssetAllocation(address asset) external view returns (
        uint256 balance,
        uint256 targetAllocation,
        uint256 currentAllocation,
        uint256 riskScore
    ) {
        Asset storage assetInfo = assets[asset];
        return (
            assetInfo.balance,
            assetInfo.targetAllocation,
            assetInfo.currentAllocation,
            assetInfo.riskScore
        );
    }
    
    /**
     * @dev Check if diversification rules are met
     */
    function checkDiversification() external view returns (bool) {
        // Implementation would check current allocations against rules
        return true; // Simplified
    }
    
    // Internal functions
    
    function _executeSpending(
        address asset,
        uint256 amount,
        address recipient
    ) internal returns (bool) {
        if (asset == address(0)) {
            // Native token (ETH)
            require(address(this).balance >= amount, "TreasuryManager: Insufficient ETH balance");
            payable(recipient).transfer(amount);
        } else {
            // ERC20 token
            IERC20 token = IERC20(asset);
            require(token.balanceOf(address(this)) >= amount, "TreasuryManager: Insufficient token balance");
            token.transfer(recipient, amount);
        }
        
        _updateAssetBalance(asset, _getAssetBalance(asset) - amount);
        return true;
    }
    
    function _getAssetBalance(address asset) internal view returns (uint256) {
        if (asset == address(0)) {
            return address(this).balance;
        } else {
            return IERC20(asset).balanceOf(address(this));
        }
    }
    
    function _updateAssetBalance(address asset, uint256 newBalance) internal {
        assets[asset].balance = newBalance;
        assets[asset].lastUpdated = block.timestamp;
        
        emit AssetUpdated(asset, newBalance, assets[asset].currentAllocation);
    }
    
    function _updateTotalTreasuryValue() internal {
        uint256 totalValue = 0;
        
        for (uint256 i = 0; i < assetList.length; i++) {
            totalValue += assets[assetList[i]].balance;
        }
        
        totalTreasuryValue = totalValue;
    }
    
    function _validateDiversification(address asset, uint256 amount) internal view {
        Asset storage assetInfo = assets[asset];
        uint256 newBalance = assetInfo.balance + amount;
        uint256 newAllocation = (newBalance * 10000) / totalTreasuryValue;
        
        require(
            newAllocation <= diversificationRule.maxSingleAssetPercentage,
            "TreasuryManager: Violates single asset limit"
        );
        
        if (assetInfo.riskScore > 70) {
            // High risk asset check (simplified)
            require(
                newAllocation <= diversificationRule.maxHighRiskPercentage,
                "TreasuryManager: Violates high risk asset limit"
            );
        }
    }
    
    function _setupDiversificationRules() internal {
        diversificationRule = DiversificationRule({
            maxSingleAssetPercentage: 3000,    // 30%
            maxHighRiskPercentage: 2000,       // 20%
            minStablePercentage: 3000,         // 30%
            maxYieldStrategyPercentage: 5000   // 50%
        });
    }
}