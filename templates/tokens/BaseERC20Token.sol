// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import "@openzeppelin/contracts/token/ERC20/ERC20.sol";
import "@openzeppelin/contracts/token/ERC20/extensions/ERC20Permit.sol";
import "@openzeppelin/contracts/token/ERC20/extensions/ERC20Votes.sol";
import "@openzeppelin/contracts/token/ERC20/extensions/ERC20Burnable.sol";
import "@openzeppelin/contracts/utils/Nonces.sol";
import "../base/SecurityBase.sol";

/**
 * @title BaseERC20Token
 * @dev A feature-rich ERC20 token template with governance and security features
 * 
 * Features:
 * - ERC20 standard compliance
 * - EIP-2612 permit functionality
 * - Governance voting capabilities
 * - Burnable tokens
 * - Comprehensive security features
 * - Configurable supply mechanics
 * - Role-based minting controls
 * 
 * Use Cases:
 * - Governance tokens
 * - Utility tokens
 * - Reward tokens
 * - Staking tokens
 * 
 * Customization:
 * Override _update() for custom transfer logic (OpenZeppelin v5 pattern)
 * Add custom minting logic in derived contracts
 * Implement additional security checks in modifiers
 */
contract BaseERC20Token is ERC20, ERC20Permit, ERC20Votes, ERC20Burnable, SecurityBase {
    // Supply management
    uint256 public immutable maxSupply;
    bool public mintingFinished;
    
    // Transfer controls
    mapping(address => bool) public blacklisted;
    mapping(address => bool) public whitelisted;
    bool public transferRestrictionsEnabled;
    
    // Fee mechanism
    uint256 public transferFeeRate; // in basis points (1% = 100)
    address public feeRecipient;
    mapping(address => bool) public feeExempt;
    
    // Roles for minting
    bytes32 public constant MINTER_ROLE = keccak256("MINTER_ROLE");
    bytes32 public constant BLACKLISTER_ROLE = keccak256("BLACKLISTER_ROLE");
    
    // Events
    event MintingFinished(uint256 timestamp);
    event TransferRestrictionsUpdated(bool enabled);
    event AddressBlacklisted(address indexed account, bool blacklisted);
    event AddressWhitelisted(address indexed account, bool whitelisted);
    event TransferFeeUpdated(uint256 oldRate, uint256 newRate);
    event FeeRecipientUpdated(address indexed oldRecipient, address indexed newRecipient);
    
    // Errors
    error MintingAlreadyFinished();
    error ExceedsMaxSupply();
    error AddressIsBlacklisted();
    error TransferNotAllowed();
    error InvalidFeeRate();
    
    modifier notBlacklisted(address account) {
        if (blacklisted[account]) {
            revert AddressIsBlacklisted();
        }
        _;
    }
    
    modifier canTransfer(address from, address to) {
        if (transferRestrictionsEnabled && !whitelisted[from] && !whitelisted[to]) {
            revert TransferNotAllowed();
        }
        _;
    }
    
    constructor(
        string memory name,
        string memory symbol,
        uint8 decimals,
        uint256 _maxSupply,
        uint256 _initialSupply,
        address initialOwner
    ) 
        ERC20(name, symbol) 
        ERC20Permit(name) 
        SecurityBase(initialOwner)
    {
        maxSupply = _maxSupply;
        feeRecipient = initialOwner;
        
        // Grant minter role to owner
        _grantRole(MINTER_ROLE, initialOwner);
        _grantRole(BLACKLISTER_ROLE, initialOwner);
        
        // Mint initial supply
        if (_initialSupply > 0) {
            _mint(initialOwner, _initialSupply);
        }
        
        // Make owner fee exempt
        feeExempt[initialOwner] = true;
    }
    
    /**
     * @dev Mint tokens to specified address
     * Can only be called by accounts with MINTER_ROLE
     */
    function mint(address to, uint256 amount) 
        external 
        onlyRole(MINTER_ROLE) 
        whenNotPaused 
        notInEmergency 
    {
        if (mintingFinished) {
            revert MintingAlreadyFinished();
        }
        
        if (totalSupply() + amount > maxSupply) {
            revert ExceedsMaxSupply();
        }
        
        _mint(to, amount);
    }
    
    /**
     * @dev Finish minting permanently
     * Can only be called by admin
     */
    function finishMinting() external onlyRole(ADMIN_ROLE) {
        mintingFinished = true;
        emit MintingFinished(block.timestamp);
    }
    
    /**
     * @dev Blacklist/unblacklist an address
     */
    function setBlacklisted(address account, bool _blacklisted) 
        external 
        onlyRole(BLACKLISTER_ROLE) 
    {
        blacklisted[account] = _blacklisted;
        emit AddressBlacklisted(account, _blacklisted);
    }
    
    /**
     * @dev Whitelist/unwhitelist an address
     */
    function setWhitelisted(address account, bool _whitelisted) 
        external 
        onlyRole(ADMIN_ROLE) 
    {
        whitelisted[account] = _whitelisted;
        emit AddressWhitelisted(account, _whitelisted);
    }
    
    /**
     * @dev Enable/disable transfer restrictions
     */
    function setTransferRestrictions(bool enabled) 
        external 
        onlyRole(ADMIN_ROLE) 
    {
        transferRestrictionsEnabled = enabled;
        emit TransferRestrictionsUpdated(enabled);
    }
    
    /**
     * @dev Set transfer fee rate (in basis points)
     */
    function setTransferFeeRate(uint256 rate) external onlyRole(ADMIN_ROLE) {
        if (rate > 1000) { // Max 10%
            revert InvalidFeeRate();
        }
        
        uint256 oldRate = transferFeeRate;
        transferFeeRate = rate;
        emit TransferFeeUpdated(oldRate, rate);
    }
    
    /**
     * @dev Set fee recipient address
     */
    function setFeeRecipient(address recipient) external onlyRole(ADMIN_ROLE) {
        _validateAddress(recipient);
        
        address oldRecipient = feeRecipient;
        feeRecipient = recipient;
        emit FeeRecipientUpdated(oldRecipient, recipient);
    }
    
    /**
     * @dev Set fee exemption status for an address
     */
    function setFeeExempt(address account, bool exempt) external onlyRole(ADMIN_ROLE) {
        feeExempt[account] = exempt;
    }
    
    /**
     * @dev Calculate transfer fee
     */
    function calculateTransferFee(uint256 amount) public view returns (uint256) {
        return (amount * transferFeeRate) / 10000;
    }
    
    /**
     * @dev Override transfer to include fees and restrictions
     */
    function _update(
        address from,
        address to,
        uint256 value
    ) 
        internal 
        override(ERC20, ERC20Votes) 
        whenNotPaused
        notBlacklisted(from)
        notBlacklisted(to)
        canTransfer(from, to)
    {
        // Handle minting (from == address(0))
        if (from == address(0)) {
            super._update(from, to, value);
            return;
        }
        
        // Handle burning (to == address(0))
        if (to == address(0)) {
            super._update(from, to, value);
            return;
        }
        
        // Handle regular transfers with fees
        uint256 fee = 0;
        if (transferFeeRate > 0 && !feeExempt[from] && !feeExempt[to]) {
            fee = calculateTransferFee(value);
            
            if (fee > 0) {
                // Transfer fee to recipient
                super._update(from, feeRecipient, fee);
            }
        }
        
        // Transfer remaining amount
        uint256 transferAmount = value - fee;
        super._update(from, to, transferAmount);
    }
    
    /**
     * @dev Get token information
     */
    function getTokenInfo() external view returns (
        string memory tokenName,
        string memory tokenSymbol,
        uint8 tokenDecimals,
        uint256 tokenTotalSupply,
        uint256 tokenMaxSupply,
        bool isMintingFinished
    ) {
        return (
            name(),
            symbol(),
            decimals(),
            totalSupply(),
            maxSupply,
            mintingFinished
        );
    }
    
    /**
     * @dev Get transfer restrictions info
     */
    function getTransferInfo() external view returns (
        bool restrictionsEnabled,
        uint256 feeRate,
        address feeRecipientAddress
    ) {
        return (
            transferRestrictionsEnabled,
            transferFeeRate,
            feeRecipient
        );
    }
    
    /**
     * @dev Override nonces function for ERC20Permit
     */
    function nonces(address owner) 
        public 
        view 
        override(ERC20Permit, Nonces) 
        returns (uint256) 
    {
        return super.nonces(owner);
    }
    
    /**
     * @dev Override decimals if needed
     */
    function decimals() public view virtual override returns (uint8) {
        return 18; // Can be overridden in derived contracts
    }
    
    /**
     * @dev Batch transfer function for efficiency
     */
    function batchTransfer(
        address[] calldata recipients, 
        uint256[] calldata amounts
    ) external returns (bool) {
        require(recipients.length == amounts.length, "BaseERC20Token: Arrays length mismatch");
        require(recipients.length <= 100, "BaseERC20Token: Too many recipients");
        
        for (uint256 i = 0; i < recipients.length; i++) {
            transfer(recipients[i], amounts[i]);
        }
        
        return true;
    }
    
    /**
     * @dev Emergency recovery for stuck tokens
     */
    function recoverERC20(address tokenAddress, uint256 amount) 
        external 
        onlyRole(ADMIN_ROLE) 
    {
        require(tokenAddress != address(this), "BaseERC20Token: Cannot recover own token");
        IERC20(tokenAddress).transfer(owner(), amount);
    }
}