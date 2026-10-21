// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import "../base/SecurityBase.sol";
import "../tokens/BaseERC20Token.sol";

/**
 * @title IERC3156FlashBorrower
 * @dev Interface for flash loan borrowers
 */
interface IERC3156FlashBorrower {
    function onFlashLoan(
        address initiator,
        address token,
        uint256 amount,
        uint256 fee,
        bytes calldata data
    ) external returns (bytes32);
}

/**
 * @title FlashLoan
 * @dev Flash loan functionality for instant, uncollateralized loans
 * 
 * Features:
 * - ERC-3156 compliant flash loan implementation
 * - Multiple token support
 * - Dynamic fee calculation
 * - Flash loan receiver interface
 * - Reentrancy protection
 * - Maximum loan limits
 * 
 * Use Cases:
 * - Arbitrage opportunities
 * - Debt refinancing
 * - Collateral swapping
 * - Liquidation protection
 */
contract FlashLoan is SecurityBase {
    // ERC-3156 interface
    bytes32 public constant CALLBACK_SUCCESS = keccak256("ERC3156FlashBorrower.onFlashLoan");
    
    struct FlashLoanConfig {
        BaseERC20Token token;        // Token available for flash loans
        uint256 maxLoanAmount;       // Maximum loan amount
        uint256 feeRate;             // Fee rate in basis points
        uint256 totalLoaned;         // Total amount loaned (for statistics)
        uint256 totalFees;           // Total fees collected
        bool active;                 // Whether flash loans are active for this token
    }
    
    struct FlashLoanData {
        address borrower;
        address token;
        uint256 amount;
        uint256 fee;
        bytes data;
    }
    
    // Flash loan configurations
    mapping(address => FlashLoanConfig) public flashLoanConfigs;
    address[] public supportedTokens;
    
    // Fee management
    address public feeRecipient;
    uint256 public defaultFeeRate = 30; // 0.3% default fee rate
    uint256 public maxFeeRate = 1000; // 10% maximum fee rate
    
    // Loan limits and controls
    mapping(address => uint256) public userLoanLimits; // Per-user loan limits
    mapping(address => uint256) public userLoanCounts; // Number of loans taken by user
    uint256 public globalDailyLimit; // Global daily loan limit
    uint256 public dailyLoaned; // Amount loaned today
    uint256 public lastDailyReset; // Last time daily limit was reset
    
    // Circuit breaker
    bool public flashLoansEnabled = true;
    uint256 public flashLoanEmergencyDelay; // Delay before emergency functions can be called
    uint256 public emergencyActivatedAt;
    
    // Monitoring
    mapping(address => uint256) public lastLoanTime; // Last loan time for each user
    mapping(address => bool) public blacklistedBorrowers; // Blacklisted addresses
    uint256 public minTimeBetweenLoans = 1 minutes; // Minimum time between loans
    
    // Events
    event FlashLoan(address indexed borrower, address indexed token, uint256 amount, uint256 fee);
    event TokenAdded(address indexed token, uint256 maxLoanAmount, uint256 feeRate);
    event TokenUpdated(address indexed token, uint256 maxLoanAmount, uint256 feeRate);
    event FeesWithdrawn(address indexed token, uint256 amount);
    event BorrowerBlacklisted(address indexed borrower);
    event EmergencyActivated(uint256 timestamp);
    
    // Errors
    error FlashLoansDisabled();
    error TokenNotSupported(address token);
    error LoanAmountTooLarge(uint256 requested, uint256 maximum);
    error InsufficientTokenBalance(uint256 available, uint256 requested);
    error FlashLoanCallbackFailed();
    error LoanNotRepaid(uint256 expected, uint256 actual);
    error BorrowerIsBlacklisted(address borrower);
    error LoanTooFrequent(uint256 timeLeft);
    error DailyLimitExceeded(uint256 requested, uint256 remaining);
    error UserLimitExceeded(address user, uint256 requested, uint256 limit);
    error InvalidFeeRate(uint256 feeRate);
    
    constructor(address initialOwner) SecurityBase(initialOwner) {
        feeRecipient = initialOwner;
        globalDailyLimit = 1000000 * 10**18; // 1M tokens daily limit
        lastDailyReset = block.timestamp;
        flashLoanEmergencyDelay = 24 hours;
    }
    
    /**
     * @dev Execute a flash loan
     */
    function flashLoan(
        IERC3156FlashBorrower receiver,
        address token,
        uint256 amount,
        bytes calldata data
    ) external nonReentrant whenNotPaused returns (bool) {
        if (!flashLoansEnabled) {
            revert FlashLoansDisabled();
        }
        
        if (blacklistedBorrowers[msg.sender]) {
            revert BorrowerIsBlacklisted(msg.sender);
        }
        
        // Check rate limiting
        if (block.timestamp < lastLoanTime[msg.sender] + minTimeBetweenLoans) {
            revert LoanTooFrequent(
                lastLoanTime[msg.sender] + minTimeBetweenLoans - block.timestamp
            );
        }
        
        FlashLoanConfig storage config = flashLoanConfigs[token];
        if (!config.active) {
            revert TokenNotSupported(token);
        }
        
        if (amount > config.maxLoanAmount) {
            revert LoanAmountTooLarge(amount, config.maxLoanAmount);
        }
        
        // Check daily limits
        _checkDailyLimits(amount);
        
        // Check user limits
        if (userLoanLimits[msg.sender] > 0 && amount > userLoanLimits[msg.sender]) {
            revert UserLimitExceeded(msg.sender, amount, userLoanLimits[msg.sender]);
        }
        
        // Check token balance
        uint256 balance = config.token.balanceOf(address(this));
        if (balance < amount) {
            revert InsufficientTokenBalance(balance, amount);
        }
        
        // Calculate fee
        uint256 fee = (amount * config.feeRate) / 10000;
        
        // Record loan details
        lastLoanTime[msg.sender] = block.timestamp;
        userLoanCounts[msg.sender]++;
        config.totalLoaned += amount;
        config.totalFees += fee;
        dailyLoaned += amount;
        
        // Transfer tokens to borrower
        config.token.transfer(address(receiver), amount);
        
        // Call the receiver
        bytes32 result = receiver.onFlashLoan(msg.sender, token, amount, fee, data);
        if (result != CALLBACK_SUCCESS) {
            revert FlashLoanCallbackFailed();
        }
        
        // Check repayment
        uint256 balanceAfter = config.token.balanceOf(address(this));
        uint256 expectedBalance = balance + fee;
        
        if (balanceAfter < expectedBalance) {
            revert LoanNotRepaid(expectedBalance, balanceAfter);
        }
        
        emit FlashLoan(msg.sender, token, amount, fee);
        return true;
    }
    
    /**
     * @dev Get flash loan fee for amount
     */
    function flashFee(address token, uint256 amount) external view returns (uint256) {
        FlashLoanConfig storage config = flashLoanConfigs[token];
        if (!config.active) {
            revert TokenNotSupported(token);
        }
        
        return (amount * config.feeRate) / 10000;
    }
    
    /**
     * @dev Get maximum flash loan amount for token
     */
    function maxFlashLoan(address token) external view returns (uint256) {
        FlashLoanConfig storage config = flashLoanConfigs[token];
        if (!config.active) {
            return 0;
        }
        
        uint256 balance = config.token.balanceOf(address(this));
        return balance < config.maxLoanAmount ? balance : config.maxLoanAmount;
    }
    
    /**
     * @dev Add support for a new token
     */
    function addFlashLoanToken(
        BaseERC20Token token,
        uint256 maxLoanAmount,
        uint256 feeRate
    ) external onlyRole(ADMIN_ROLE) {
        if (feeRate > maxFeeRate) {
            revert InvalidFeeRate(feeRate);
        }
        
        address tokenAddress = address(token);
        
        // Add to supported tokens if not already present
        if (!flashLoanConfigs[tokenAddress].active) {
            supportedTokens.push(tokenAddress);
        }
        
        flashLoanConfigs[tokenAddress] = FlashLoanConfig({
            token: token,
            maxLoanAmount: maxLoanAmount,
            feeRate: feeRate,
            totalLoaned: 0,
            totalFees: 0,
            active: true
        });
        
        emit TokenAdded(tokenAddress, maxLoanAmount, feeRate);
    }
    
    /**
     * @dev Update flash loan configuration for token
     */
    function updateFlashLoanToken(
        address token,
        uint256 maxLoanAmount,
        uint256 feeRate,
        bool active
    ) external onlyRole(ADMIN_ROLE) {
        if (feeRate > maxFeeRate) {
            revert InvalidFeeRate(feeRate);
        }
        
        FlashLoanConfig storage config = flashLoanConfigs[token];
        require(address(config.token) != address(0), "FlashLoan: Token not found");
        
        config.maxLoanAmount = maxLoanAmount;
        config.feeRate = feeRate;
        config.active = active;
        
        emit TokenUpdated(token, maxLoanAmount, feeRate);
    }
    
    /**
     * @dev Withdraw collected fees
     */
    function withdrawFees(address token) external onlyRole(ADMIN_ROLE) {
        FlashLoanConfig storage config = flashLoanConfigs[token];
        require(config.active, "FlashLoan: Token not supported");
        
        uint256 feeAmount = config.totalFees;
        require(feeAmount > 0, "FlashLoan: No fees to withdraw");
        
        config.totalFees = 0;
        config.token.transfer(feeRecipient, feeAmount);
        
        emit FeesWithdrawn(token, feeAmount);
    }
    
    /**
     * @dev Withdraw all fees from all supported tokens
     */
    function withdrawAllFees() external onlyRole(ADMIN_ROLE) {
        for (uint256 i = 0; i < supportedTokens.length; i++) {
            address token = supportedTokens[i];
            FlashLoanConfig storage config = flashLoanConfigs[token];
            
            if (config.totalFees > 0) {
                uint256 feeAmount = config.totalFees;
                config.totalFees = 0;
                config.token.transfer(feeRecipient, feeAmount);
                
                emit FeesWithdrawn(token, feeAmount);
            }
        }
    }
    
    /**
     * @dev Set user loan limit
     */
    function setUserLoanLimit(address user, uint256 limit) external onlyRole(ADMIN_ROLE) {
        userLoanLimits[user] = limit;
    }
    
    /**
     * @dev Blacklist a borrower
     */
    function blacklistBorrower(address borrower, bool blacklisted) external onlyRole(ADMIN_ROLE) {
        blacklistedBorrowers[borrower] = blacklisted;
        if (blacklisted) {
            emit BorrowerBlacklisted(borrower);
        }
    }
    
    /**
     * @dev Set global daily limit
     */
    function setGlobalDailyLimit(uint256 limit) external onlyRole(ADMIN_ROLE) {
        globalDailyLimit = limit;
    }
    
    /**
     * @dev Set minimum time between loans
     */
    function setMinTimeBetweenLoans(uint256 time) external onlyRole(ADMIN_ROLE) {
        minTimeBetweenLoans = time;
    }
    
    /**
     * @dev Enable/disable flash loans
     */
    function setFlashLoansEnabled(bool enabled) external onlyRole(ADMIN_ROLE) {
        flashLoansEnabled = enabled;
    }
    
    /**
     * @dev Set fee recipient
     */
    function setFeeRecipient(address newFeeRecipient) external onlyRole(ADMIN_ROLE) {
        require(newFeeRecipient != address(0), "FlashLoan: Invalid fee recipient");
        feeRecipient = newFeeRecipient;
    }
    
    /**
     * @dev Set default fee rate
     */
    function setDefaultFeeRate(uint256 feeRate) external onlyRole(ADMIN_ROLE) {
        if (feeRate > maxFeeRate) {
            revert InvalidFeeRate(feeRate);
        }
        defaultFeeRate = feeRate;
    }
    
    /**
     * @dev Emergency pause (can be called by emergency role)
     */
    function emergencyPause() external onlyRole(EMERGENCY_ROLE) {
        flashLoansEnabled = false;
        emergencyActivatedAt = block.timestamp;
        emit EmergencyActivated(block.timestamp);
    }
    
    /**
     * @dev Emergency withdraw (after emergency delay)
     */
    function emergencyWithdraw(address token, uint256 amount) external onlyRole(EMERGENCY_ROLE) {
        require(
            emergencyActivatedAt > 0 && 
            block.timestamp >= emergencyActivatedAt + flashLoanEmergencyDelay,
            "FlashLoan: Emergency delay not met"
        );
        
        BaseERC20Token(token).transfer(emergencyRecovery, amount);
    }
    
    /**
     * @dev Get supported tokens
     */
    function getSupportedTokens() external view returns (address[] memory) {
        return supportedTokens;
    }
    
    /**
     * @dev Get flash loan statistics for token
     */
    function getFlashLoanStats(address token) external view returns (
        uint256 totalLoaned,
        uint256 totalFees,
        uint256 maxLoanAmount,
        uint256 feeRate,
        bool active
    ) {
        FlashLoanConfig storage config = flashLoanConfigs[token];
        return (
            config.totalLoaned,
            config.totalFees,
            config.maxLoanAmount,
            config.feeRate,
            config.active
        );
    }
    
    /**
     * @dev Get user loan statistics
     */
    function getUserLoanStats(address user) external view returns (
        uint256 loanCount,
        uint256 lastLoanTime,
        uint256 loanLimit,
        bool blacklisted
    ) {
        return (
            userLoanCounts[user],
            lastLoanTime[user],
            userLoanLimits[user],
            blacklistedBorrowers[user]
        );
    }
    
    /**
     * @dev Check daily limits
     */
    function _checkDailyLimits(uint256 amount) internal {
        // Reset daily counter if needed
        if (block.timestamp >= lastDailyReset + 1 days) {
            dailyLoaned = 0;
            lastDailyReset = block.timestamp;
        }
        
        // Check global daily limit
        if (dailyLoaned + amount > globalDailyLimit) {
            revert DailyLimitExceeded(amount, globalDailyLimit - dailyLoaned);
        }
    }
    
    /**
     * @dev Get remaining daily limit
     */
    function getRemainingDailyLimit() external view returns (uint256) {
        // Check if we need to reset
        if (block.timestamp >= lastDailyReset + 1 days) {
            return globalDailyLimit;
        }
        
        return globalDailyLimit > dailyLoaned ? globalDailyLimit - dailyLoaned : 0;
    }
    
    /**
     * @dev Check if user can take a loan
     */
    function canTakeLoan(address user, address token, uint256 amount) external view returns (bool, string memory) {
        if (!flashLoansEnabled) {
            return (false, "Flash loans disabled");
        }
        
        if (blacklistedBorrowers[user]) {
            return (false, "User blacklisted");
        }
        
        FlashLoanConfig storage config = flashLoanConfigs[token];
        if (!config.active) {
            return (false, "Token not supported");
        }
        
        if (amount > config.maxLoanAmount) {
            return (false, "Amount exceeds maximum");
        }
        
        if (userLoanLimits[user] > 0 && amount > userLoanLimits[user]) {
            return (false, "Amount exceeds user limit");
        }
        
        // Check daily limits
        uint256 remainingDaily = this.getRemainingDailyLimit();
        if (amount > remainingDaily) {
            return (false, "Amount exceeds daily limit");
        }
        
        // Check rate limiting
        if (block.timestamp < lastLoanTime[user] + minTimeBetweenLoans) {
            return (false, "Loan too frequent");
        }
        
        // Check balance
        uint256 balance = config.token.balanceOf(address(this));
        if (balance < amount) {
            return (false, "Insufficient liquidity");
        }
        
        return (true, "");
    }
}