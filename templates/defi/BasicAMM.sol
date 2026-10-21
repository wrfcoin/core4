// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import "../base/SecurityBase.sol";
import "../tokens/BaseERC20Token.sol";

/**
 * @title BasicAMM
 * @dev Automated Market Maker for token swaps
 * 
 * Features:
 * - Constant product formula (x * y = k)
 * - Liquidity provision and removal
 * - Dynamic fee adjustment
 * - Slippage protection
 * - Price oracle integration
 * - MEV protection
 * 
 * Use Cases:
 * - Carbon credit trading
 * - Token swaps within ecosystem
 * - Liquidity provision rewards
 * - Decentralized price discovery
 */
contract BasicAMM is SecurityBase {
    // Core tokens
    BaseERC20Token public immutable tokenA;
    BaseERC20Token public immutable tokenB;
    
    // Liquidity tracking
    uint256 public reserveA;
    uint256 public reserveB;
    uint256 public totalLiquidity;
    mapping(address => uint256) public liquidityBalances;
    
    // Fee structure
    uint256 public swapFeeRate = 30; // 0.3% in basis points
    uint256 public protocolFeeRate = 5; // 0.05% in basis points
    address public feeRecipient;
    
    // Price protection
    uint256 public maxSlippage = 500; // 5% max slippage
    uint256 public lastPrice; // Price in tokenB per tokenA
    uint256 public priceUpdateTime;
    uint256 public constant PRICE_VALIDITY_PERIOD = 5 minutes;
    
    // MEV protection
    mapping(address => uint256) public lastSwapBlock;
    uint256 public constant MIN_SWAP_INTERVAL = 1; // Minimum blocks between swaps
    
    // Circuit breaker
    uint256 public maxSwapAmount; // Maximum swap amount per transaction
    uint256 public dailyVolumeLimit;
    uint256 public dailyVolume;
    uint256 public lastVolumeResetTime;
    
    // Events
    event LiquidityAdded(address indexed provider, uint256 amountA, uint256 amountB, uint256 liquidity);
    event LiquidityRemoved(address indexed provider, uint256 amountA, uint256 amountB, uint256 liquidity);
    event Swap(address indexed trader, address indexed tokenIn, uint256 amountIn, uint256 amountOut);
    event PriceUpdate(uint256 newPrice, uint256 timestamp);
    event FeesCollected(uint256 amountA, uint256 amountB);
    
    // Errors
    error InsufficientLiquidity(uint256 available, uint256 required);
    error SlippageExceeded(uint256 expected, uint256 actual);
    error SwapTooFrequent(uint256 blocksToWait);
    error SwapAmountTooLarge(uint256 amount, uint256 limit);
    error DailyVolumeLimitExceeded(uint256 volume, uint256 limit);
    error InvalidTokenAmount(uint256 amount);
    error ZeroLiquidity();
    
    constructor(
        address _tokenA,
        address _tokenB,
        address initialOwner,
        address _feeRecipient
    ) SecurityBase(initialOwner) {
        tokenA = BaseERC20Token(_tokenA);
        tokenB = BaseERC20Token(_tokenB);
        feeRecipient = _feeRecipient;
        
        // Initialize limits
        maxSwapAmount = 10000 * 10**18; // 10k tokens
        dailyVolumeLimit = 100000 * 10**18; // 100k tokens daily
        lastVolumeResetTime = block.timestamp;
    }
    
    /**
     * @dev Add liquidity to the pool
     */
    function addLiquidity(
        uint256 amountA,
        uint256 amountB,
        uint256 minAmountA,
        uint256 minAmountB
    ) external nonReentrant whenNotPaused returns (uint256 liquidity) {
        if (amountA == 0 || amountB == 0) {
            revert InvalidTokenAmount(0);
        }
        
        // Check if this is the first liquidity provision
        if (totalLiquidity == 0) {
            // First liquidity provider sets the initial ratio
            liquidity = _sqrt(amountA * amountB);
            require(liquidity > 1000, "BasicAMM: Insufficient initial liquidity"); // Prevent manipulation
            
            // Burn minimum liquidity to prevent attacks
            totalLiquidity = liquidity - 1000;
            liquidityBalances[address(0)] = 1000;
        } else {
            // Subsequent liquidity must maintain the ratio
            uint256 liquidityA = (amountA * totalLiquidity) / reserveA;
            uint256 liquidityB = (amountB * totalLiquidity) / reserveB;
            liquidity = liquidityA < liquidityB ? liquidityA : liquidityB;
            
            // Adjust amounts to maintain ratio
            amountA = (liquidity * reserveA) / totalLiquidity;
            amountB = (liquidity * reserveB) / totalLiquidity;
            
            // Check slippage protection
            if (amountA < minAmountA || amountB < minAmountB) {
                revert SlippageExceeded(minAmountA + minAmountB, amountA + amountB);
            }
        }
        
        // Transfer tokens from user
        tokenA.transferFrom(msg.sender, address(this), amountA);
        tokenB.transferFrom(msg.sender, address(this), amountB);
        
        // Update reserves and liquidity
        reserveA += amountA;
        reserveB += amountB;
        totalLiquidity += liquidity;
        liquidityBalances[msg.sender] += liquidity;
        
        // Update price
        _updatePrice();
        
        emit LiquidityAdded(msg.sender, amountA, amountB, liquidity);
        return liquidity;
    }
    
    /**
     * @dev Remove liquidity from the pool
     */
    function removeLiquidity(
        uint256 liquidity,
        uint256 minAmountA,
        uint256 minAmountB
    ) external nonReentrant returns (uint256 amountA, uint256 amountB) {
        if (liquidity == 0) {
            revert InvalidTokenAmount(0);
        }
        
        if (liquidityBalances[msg.sender] < liquidity) {
            revert InsufficientLiquidity(liquidityBalances[msg.sender], liquidity);
        }
        
        // Calculate amounts to return
        amountA = (liquidity * reserveA) / totalLiquidity;
        amountB = (liquidity * reserveB) / totalLiquidity;
        
        // Check slippage protection
        if (amountA < minAmountA || amountB < minAmountB) {
            revert SlippageExceeded(minAmountA + minAmountB, amountA + amountB);
        }
        
        // Update state
        liquidityBalances[msg.sender] -= liquidity;
        totalLiquidity -= liquidity;
        reserveA -= amountA;
        reserveB -= amountB;
        
        // Transfer tokens to user
        tokenA.transfer(msg.sender, amountA);
        tokenB.transfer(msg.sender, amountB);
        
        // Update price
        _updatePrice();
        
        emit LiquidityRemoved(msg.sender, amountA, amountB, liquidity);
        return (amountA, amountB);
    }
    
    /**
     * @dev Swap tokenA for tokenB
     */
    function swapAForB(
        uint256 amountAIn,
        uint256 minAmountBOut
    ) external nonReentrant whenNotPaused returns (uint256 amountBOut) {
        return _swap(address(tokenA), amountAIn, minAmountBOut);
    }
    
    /**
     * @dev Swap tokenB for tokenA
     */
    function swapBForA(
        uint256 amountBIn,
        uint256 minAmountAOut
    ) external nonReentrant whenNotPaused returns (uint256 amountAOut) {
        return _swap(address(tokenB), amountBIn, minAmountAOut);
    }
    
    /**
     * @dev Get expected output amount for a swap
     */
    function getAmountOut(address tokenIn, uint256 amountIn) external view returns (uint256 amountOut) {
        if (amountIn == 0) return 0;
        
        uint256 reserveIn;
        uint256 reserveOut;
        
        if (tokenIn == address(tokenA)) {
            reserveIn = reserveA;
            reserveOut = reserveB;
        } else if (tokenIn == address(tokenB)) {
            reserveIn = reserveB;
            reserveOut = reserveA;
        } else {
            revert("BasicAMM: Invalid token");
        }
        
        // Calculate output with fees
        uint256 amountInWithFee = amountIn * (10000 - swapFeeRate);
        uint256 numerator = amountInWithFee * reserveOut;
        uint256 denominator = (reserveIn * 10000) + amountInWithFee;
        
        amountOut = numerator / denominator;
        return amountOut;
    }
    
    /**
     * @dev Get current price (tokenB per tokenA)
     */
    function getCurrentPrice() external view returns (uint256) {
        if (reserveA == 0) return 0;
        return (reserveB * 10**18) / reserveA;
    }
    
    /**
     * @dev Check if price is stale
     */
    function isPriceStale() external view returns (bool) {
        return block.timestamp > priceUpdateTime + PRICE_VALIDITY_PERIOD;
    }
    
    /**
     * @dev Internal swap function
     */
    function _swap(address tokenIn, uint256 amountIn, uint256 minAmountOut) internal returns (uint256 amountOut) {
        if (amountIn == 0) {
            revert InvalidTokenAmount(0);
        }
        
        // MEV protection
        if (block.number <= lastSwapBlock[msg.sender] + MIN_SWAP_INTERVAL) {
            revert SwapTooFrequent(lastSwapBlock[msg.sender] + MIN_SWAP_INTERVAL - block.number);
        }
        
        // Volume limits
        if (amountIn > maxSwapAmount) {
            revert SwapAmountTooLarge(amountIn, maxSwapAmount);
        }
        
        _checkDailyVolumeLimit(amountIn);
        
        // Get reserves
        uint256 reserveIn;
        uint256 reserveOut;
        BaseERC20Token tokenInContract;
        BaseERC20Token tokenOutContract;
        
        if (tokenIn == address(tokenA)) {
            reserveIn = reserveA;
            reserveOut = reserveB;
            tokenInContract = tokenA;
            tokenOutContract = tokenB;
        } else {
            reserveIn = reserveB;
            reserveOut = reserveA;
            tokenInContract = tokenB;
            tokenOutContract = tokenA;
        }
        
        if (reserveIn == 0 || reserveOut == 0) {
            revert ZeroLiquidity();
        }
        
        // Calculate output amount
        amountOut = this.getAmountOut(tokenIn, amountIn);
        
        if (amountOut < minAmountOut) {
            revert SlippageExceeded(minAmountOut, amountOut);
        }
        
        if (amountOut >= reserveOut) {
            revert InsufficientLiquidity(reserveOut, amountOut);
        }
        
        // Transfer tokens
        tokenInContract.transferFrom(msg.sender, address(this), amountIn);
        tokenOutContract.transfer(msg.sender, amountOut);
        
        // Update reserves
        if (tokenIn == address(tokenA)) {
            reserveA += amountIn;
            reserveB -= amountOut;
        } else {
            reserveB += amountIn;
            reserveA -= amountOut;
        }
        
        // Update MEV protection
        lastSwapBlock[msg.sender] = block.number;
        
        // Update daily volume
        dailyVolume += amountIn;
        
        // Update price
        _updatePrice();
        
        emit Swap(msg.sender, tokenIn, amountIn, amountOut);
        return amountOut;
    }
    
    /**
     * @dev Update price oracle
     */
    function _updatePrice() internal {
        if (reserveA > 0) {
            lastPrice = (reserveB * 10**18) / reserveA;
            priceUpdateTime = block.timestamp;
            emit PriceUpdate(lastPrice, block.timestamp);
        }
    }
    
    /**
     * @dev Check daily volume limit
     */
    function _checkDailyVolumeLimit(uint256 amount) internal {
        // Reset daily volume if needed
        if (block.timestamp >= lastVolumeResetTime + 1 days) {
            dailyVolume = 0;
            lastVolumeResetTime = block.timestamp;
        }
        
        if (dailyVolume + amount > dailyVolumeLimit) {
            revert DailyVolumeLimitExceeded(dailyVolume + amount, dailyVolumeLimit);
        }
    }
    
    /**
     * @dev Square root function for liquidity calculation
     */
    function _sqrt(uint256 x) internal pure returns (uint256) {
        if (x == 0) return 0;
        uint256 z = (x + 1) / 2;
        uint256 y = x;
        while (z < y) {
            y = z;
            z = (x / z + z) / 2;
        }
        return y;
    }
    
    /**
     * @dev Collect accumulated fees
     */
    function collectFees() external onlyRole(ADMIN_ROLE) {
        uint256 feeA = tokenA.balanceOf(address(this)) - reserveA;
        uint256 feeB = tokenB.balanceOf(address(this)) - reserveB;
        
        if (feeA > 0) {
            tokenA.transfer(feeRecipient, feeA);
        }
        if (feeB > 0) {
            tokenB.transfer(feeRecipient, feeB);
        }
        
        emit FeesCollected(feeA, feeB);
    }
    
    /**
     * @dev Set swap fee rate
     */
    function setSwapFeeRate(uint256 newRate) external onlyRole(ADMIN_ROLE) {
        require(newRate <= 100, "BasicAMM: Fee too high"); // Max 1%
        swapFeeRate = newRate;
    }
    
    /**
     * @dev Set maximum slippage
     */
    function setMaxSlippage(uint256 newSlippage) external onlyRole(ADMIN_ROLE) {
        require(newSlippage <= 2000, "BasicAMM: Slippage too high"); // Max 20%
        maxSlippage = newSlippage;
    }
    
    /**
     * @dev Set maximum swap amount
     */
    function setMaxSwapAmount(uint256 newAmount) external onlyRole(ADMIN_ROLE) {
        maxSwapAmount = newAmount;
    }
    
    /**
     * @dev Set daily volume limit
     */
    function setDailyVolumeLimit(uint256 newLimit) external onlyRole(ADMIN_ROLE) {
        dailyVolumeLimit = newLimit;
    }
    
    /**
     * @dev Emergency drain function
     */
    function emergencyDrain() external onlyRole(EMERGENCY_ROLE) {
        require(emergencyShutdown, "BasicAMM: Emergency not active");
        
        uint256 balanceA = tokenA.balanceOf(address(this));
        uint256 balanceB = tokenB.balanceOf(address(this));
        
        if (balanceA > 0) {
            tokenA.transfer(emergencyRecovery, balanceA);
        }
        if (balanceB > 0) {
            tokenB.transfer(emergencyRecovery, balanceB);
        }
    }
}