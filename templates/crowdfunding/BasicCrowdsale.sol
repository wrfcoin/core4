// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import "../base/SecurityBase.sol";
import "../tokens/BaseERC20Token.sol";

/**
 * @title BasicCrowdsale
 * @dev Template for basic token crowdsale with time and cap limits
 * 
 * Features:
 * - Time-bounded sale periods
 * - Hard cap and soft cap support
 * - Multiple payment token support
 * - Automatic refunds if soft cap not met
 * - Whitelist and KYC integration
 * - Vesting schedule support
 * 
 * Use Cases:
 * - ICO/IDO launches
 * - Token presales
 * - Community fundraising
 * - Project funding rounds
 */
contract BasicCrowdsale is SecurityBase {
    using SafeMath for uint256;
    
    // Token being sold
    BaseERC20Token public immutable token;
    
    // Sale parameters
    uint256 public immutable startTime;
    uint256 public immutable endTime;
    uint256 public immutable rate; // tokens per wei
    uint256 public immutable hardCap; // max wei to raise
    uint256 public immutable softCap; // min wei to raise
    uint256 public immutable minContribution;
    uint256 public immutable maxContribution;
    
    // Sale state
    uint256 public weiRaised;
    uint256 public tokensSold;
    bool public finalized;
    bool public softCapReached;
    
    // Beneficiary management
    address payable public wallet; // where funds are forwarded
    mapping(address => uint256) public contributions;
    mapping(address => uint256) public tokenBalances;
    
    // Whitelist and KYC
    mapping(address => bool) public whitelist;
    mapping(address => bool) public kycVerified;
    bool public whitelistEnabled;
    bool public kycRequired;
    
    // Vesting
    mapping(address => uint256) public vestingStart;
    mapping(address => uint256) public vestingDuration;
    mapping(address => uint256) public vestingClaimed;
    uint256 public defaultVestingDuration = 180 days; // 6 months
    
    // Payment tokens (if supporting multiple currencies)
    struct PaymentToken {
        address tokenAddress;
        uint256 rate; // tokens per payment token unit
        bool enabled;
    }
    mapping(address => PaymentToken) public paymentTokens;
    address[] public paymentTokenList;
    
    // Events
    event TokensPurchased(
        address indexed purchaser,
        address indexed beneficiary,
        uint256 value,
        uint256 amount
    );
    event CrowdsaleFinalized(uint256 totalRaised, uint256 totalSold);
    event SoftCapReached(uint256 timestamp, uint256 amount);
    event RefundClaimed(address indexed beneficiary, uint256 amount);
    event TokensClaimed(address indexed beneficiary, uint256 amount);
    event WhitelistUpdated(address indexed account, bool whitelisted);
    event KYCUpdated(address indexed account, bool verified);
    event PaymentTokenAdded(address indexed token, uint256 rate);
    
    // Errors
    error SaleNotActive();
    error SaleEnded();
    error HardCapExceeded();
    error ContributionLimitsViolated();
    error NotWhitelisted();
    error KYCNotVerified();
    error SoftCapNotReached();
    error AlreadyFinalized();
    error NoRefundAvailable();
    error NoTokensToClaim();
    error VestingNotStarted();
    error InsufficientVestedTokens();
    error InvalidPaymentToken();
    
    modifier onlyWhileOpen() {
        if (block.timestamp < startTime || block.timestamp > endTime) {
            revert SaleNotActive();
        }
        if (finalized) {
            revert SaleEnded();
        }
        _;
    }
    
    modifier onlyFinalized() {
        if (!finalized) {
            revert SaleNotActive();
        }
        _;
    }
    
    modifier onlyKYCVerified() {
        if (kycRequired && !kycVerified[msg.sender]) {
            revert KYCNotVerified();
        }
        _;
    }
    
    modifier onlyWhitelisted() {
        if (whitelistEnabled && !whitelist[msg.sender]) {
            revert NotWhitelisted();
        }
        _;
    }
    
    constructor(
        address _token,
        uint256 _startTime,
        uint256 _endTime,
        uint256 _rate,
        uint256 _hardCap,
        uint256 _softCap,
        uint256 _minContribution,
        uint256 _maxContribution,
        address payable _wallet,
        address initialOwner
    ) SecurityBase(initialOwner) {
        require(_token != address(0), "BasicCrowdsale: Invalid token");
        require(_startTime >= block.timestamp, "BasicCrowdsale: Start time in past");
        require(_endTime > _startTime, "BasicCrowdsale: Invalid end time");
        require(_rate > 0, "BasicCrowdsale: Invalid rate");
        require(_hardCap > _softCap, "BasicCrowdsale: Hard cap must exceed soft cap");
        require(_wallet != address(0), "BasicCrowdsale: Invalid wallet");
        
        token = BaseERC20Token(_token);
        startTime = _startTime;
        endTime = _endTime;
        rate = _rate;
        hardCap = _hardCap;
        softCap = _softCap;
        minContribution = _minContribution;
        maxContribution = _maxContribution;
        wallet = _wallet;
        
        // Setup ETH as default payment method
        paymentTokens[address(0)] = PaymentToken({
            tokenAddress: address(0),
            rate: _rate,
            enabled: true
        });
        paymentTokenList.push(address(0));
    }
    
    /**
     * @dev Buy tokens with ETH
     */
    receive() external payable {
        buyTokens(msg.sender);
    }
    
    /**
     * @dev Buy tokens for a beneficiary with ETH
     */
    function buyTokens(address beneficiary) 
        public 
        payable 
        nonReentrant 
        onlyWhileOpen 
        onlyWhitelisted 
        onlyKYCVerified 
        whenNotPaused 
    {
        _buyTokensWithETH(beneficiary, msg.value);
    }
    
    /**
     * @dev Buy tokens with supported ERC20 tokens
     */
    function buyTokensWithToken(
        address paymentToken,
        uint256 amount,
        address beneficiary
    ) 
        external 
        nonReentrant 
        onlyWhileOpen 
        onlyWhitelisted 
        onlyKYCVerified 
        whenNotPaused 
    {
        PaymentToken storage payment = paymentTokens[paymentToken];
        if (!payment.enabled) {
            revert InvalidPaymentToken();
        }
        
        // Transfer payment tokens from buyer
        IERC20(paymentToken).transferFrom(msg.sender, address(this), amount);
        
        // Calculate tokens to issue
        uint256 tokens = amount.mul(payment.rate);
        
        _validateAndProcessPurchase(beneficiary, amount, tokens);
        
        emit TokensPurchased(msg.sender, beneficiary, amount, tokens);
    }
    
    /**
     * @dev Internal ETH purchase logic
     */
    function _buyTokensWithETH(address beneficiary, uint256 weiAmount) internal {
        uint256 tokens = weiAmount.mul(rate);
        
        _validateAndProcessPurchase(beneficiary, weiAmount, tokens);
        
        emit TokensPurchased(msg.sender, beneficiary, weiAmount, tokens);
    }
    
    /**
     * @dev Validate and process token purchase
     */
    function _validateAndProcessPurchase(
        address beneficiary,
        uint256 weiAmount,
        uint256 tokenAmount
    ) internal {
        _validateAddress(beneficiary);
        
        // Check contribution limits
        uint256 newContribution = contributions[beneficiary].add(weiAmount);
        if (newContribution < minContribution || newContribution > maxContribution) {
            revert ContributionLimitsViolated();
        }
        
        // Check hard cap
        if (weiRaised.add(weiAmount) > hardCap) {
            revert HardCapExceeded();
        }
        
        // Update state
        weiRaised = weiRaised.add(weiAmount);
        tokensSold = tokensSold.add(tokenAmount);
        contributions[beneficiary] = contributions[beneficiary].add(weiAmount);
        tokenBalances[beneficiary] = tokenBalances[beneficiary].add(tokenAmount);
        
        // Check if soft cap reached
        if (!softCapReached && weiRaised >= softCap) {
            softCapReached = true;
            emit SoftCapReached(block.timestamp, weiRaised);
        }
        
        // Setup vesting for this beneficiary
        if (vestingStart[beneficiary] == 0) {
            vestingStart[beneficiary] = endTime; // Vesting starts after sale
            vestingDuration[beneficiary] = defaultVestingDuration;
        }
    }
    
    /**
     * @dev Finalize the crowdsale
     */
    function finalize() external onlyRole(ADMIN_ROLE) {
        if (block.timestamp <= endTime && weiRaised < hardCap) {
            revert SaleNotActive();
        }
        if (finalized) {
            revert AlreadyFinalized();
        }
        
        finalized = true;
        
        if (softCapReached) {
            // Forward funds to wallet
            wallet.transfer(address(this).balance);
            
            // Forward any ERC20 payment tokens
            for (uint256 i = 1; i < paymentTokenList.length; i++) {
                address paymentToken = paymentTokenList[i];
                uint256 balance = IERC20(paymentToken).balanceOf(address(this));
                if (balance > 0) {
                    IERC20(paymentToken).transfer(wallet, balance);
                }
            }
        }
        
        emit CrowdsaleFinalized(weiRaised, tokensSold);
    }
    
    /**
     * @dev Claim refund if soft cap not reached
     */
    function claimRefund() external nonReentrant onlyFinalized {
        if (softCapReached) {
            revert SoftCapNotReached();
        }
        
        uint256 refundAmount = contributions[msg.sender];
        if (refundAmount == 0) {
            revert NoRefundAvailable();
        }
        
        contributions[msg.sender] = 0;
        payable(msg.sender).transfer(refundAmount);
        
        emit RefundClaimed(msg.sender, refundAmount);
    }
    
    /**
     * @dev Claim vested tokens
     */
    function claimTokens() external nonReentrant onlyFinalized {
        if (!softCapReached) {
            revert SoftCapNotReached();
        }
        
        uint256 vestedAmount = getVestedAmount(msg.sender);
        uint256 claimableAmount = vestedAmount.sub(vestingClaimed[msg.sender]);
        
        if (claimableAmount == 0) {
            revert NoTokensToClaim();
        }
        
        vestingClaimed[msg.sender] = vestingClaimed[msg.sender].add(claimableAmount);
        token.transfer(msg.sender, claimableAmount);
        
        emit TokensClaimed(msg.sender, claimableAmount);
    }
    
    /**
     * @dev Calculate vested token amount for an address
     */
    function getVestedAmount(address beneficiary) public view returns (uint256) {
        uint256 totalTokens = tokenBalances[beneficiary];
        uint256 start = vestingStart[beneficiary];
        uint256 duration = vestingDuration[beneficiary];
        
        if (block.timestamp < start) {
            return 0;
        }
        
        if (block.timestamp >= start.add(duration)) {
            return totalTokens;
        }
        
        uint256 elapsed = block.timestamp.sub(start);
        return totalTokens.mul(elapsed).div(duration);
    }
    
    /**
     * @dev Add address to whitelist
     */
    function addToWhitelist(address[] calldata accounts) external onlyRole(ADMIN_ROLE) {
        for (uint256 i = 0; i < accounts.length; i++) {
            whitelist[accounts[i]] = true;
            emit WhitelistUpdated(accounts[i], true);
        }
    }
    
    /**
     * @dev Remove address from whitelist
     */
    function removeFromWhitelist(address[] calldata accounts) external onlyRole(ADMIN_ROLE) {
        for (uint256 i = 0; i < accounts.length; i++) {
            whitelist[accounts[i]] = false;
            emit WhitelistUpdated(accounts[i], false);
        }
    }
    
    /**
     * @dev Verify KYC for addresses
     */
    function verifyKYC(address[] calldata accounts) external onlyRole(OPERATOR_ROLE) {
        for (uint256 i = 0; i < accounts.length; i++) {
            kycVerified[accounts[i]] = true;
            emit KYCUpdated(accounts[i], true);
        }
    }
    
    /**
     * @dev Enable/disable whitelist requirement
     */
    function setWhitelistEnabled(bool enabled) external onlyRole(ADMIN_ROLE) {
        whitelistEnabled = enabled;
    }
    
    /**
     * @dev Enable/disable KYC requirement
     */
    function setKYCRequired(bool required) external onlyRole(ADMIN_ROLE) {
        kycRequired = required;
    }
    
    /**
     * @dev Add payment token
     */
    function addPaymentToken(address tokenAddress, uint256 tokenRate) 
        external 
        onlyRole(ADMIN_ROLE) 
    {
        require(tokenAddress != address(0), "BasicCrowdsale: Invalid token");
        require(tokenRate > 0, "BasicCrowdsale: Invalid rate");
        
        if (!paymentTokens[tokenAddress].enabled) {
            paymentTokenList.push(tokenAddress);
        }
        
        paymentTokens[tokenAddress] = PaymentToken({
            tokenAddress: tokenAddress,
            rate: tokenRate,
            enabled: true
        });
        
        emit PaymentTokenAdded(tokenAddress, tokenRate);
    }
    
    /**
     * @dev Set custom vesting for specific address
     */
    function setCustomVesting(
        address beneficiary,
        uint256 start,
        uint256 duration
    ) external onlyRole(ADMIN_ROLE) {
        vestingStart[beneficiary] = start;
        vestingDuration[beneficiary] = duration;
    }
    
    /**
     * @dev Emergency token recovery
     */
    function recoverTokens(address tokenAddress, uint256 amount) 
        external 
        onlyRole(ADMIN_ROLE) 
    {
        require(finalized, "BasicCrowdsale: Sale not finalized");
        
        if (tokenAddress == address(token)) {
            // Only allow recovery of unsold tokens
            uint256 contractBalance = token.balanceOf(address(this));
            uint256 unclaimedTokens = tokensSold.sub(_getTotalClaimed());
            require(amount <= contractBalance.sub(unclaimedTokens), "BasicCrowdsale: Cannot recover claimed tokens");
        }
        
        IERC20(tokenAddress).transfer(owner(), amount);
    }
    
    /**
     * @dev Get crowdsale information
     */
    function getCrowdsaleInfo() external view returns (
        uint256 currentTime,
        uint256 saleStart,
        uint256 saleEnd,
        uint256 tokensRate,
        uint256 raised,
        uint256 sold,
        uint256 hardCapAmount,
        uint256 softCapAmount,
        bool isFinalized,
        bool isSoftCapReached
    ) {
        return (
            block.timestamp,
            startTime,
            endTime,
            rate,
            weiRaised,
            tokensSold,
            hardCap,
            softCap,
            finalized,
            softCapReached
        );
    }
    
    /**
     * @dev Get user contribution info
     */
    function getUserInfo(address user) external view returns (
        uint256 contribution,
        uint256 tokenBalance,
        uint256 vestedTokens,
        uint256 claimedTokens,
        bool isWhitelisted,
        bool isKYCVerified
    ) {
        return (
            contributions[user],
            tokenBalances[user],
            getVestedAmount(user),
            vestingClaimed[user],
            whitelist[user],
            kycVerified[user]
        );
    }
    
    // Internal helper functions
    
    function _getTotalClaimed() internal view returns (uint256) {
        // This is a simplified version - in production, maintain a running total
        return 0;
    }
}

// SafeMath library for older Solidity versions (can be removed for 0.8+)
library SafeMath {
    function add(uint256 a, uint256 b) internal pure returns (uint256) {
        return a + b;
    }
    
    function sub(uint256 a, uint256 b) internal pure returns (uint256) {
        return a - b;
    }
    
    function mul(uint256 a, uint256 b) internal pure returns (uint256) {
        return a * b;
    }
    
    function div(uint256 a, uint256 b) internal pure returns (uint256) {
        return a / b;
    }
}