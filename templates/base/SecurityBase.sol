// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import "@openzeppelin/contracts/access/Ownable.sol";
import "@openzeppelin/contracts/utils/Pausable.sol";
import "@openzeppelin/contracts/utils/ReentrancyGuard.sol";
import "@openzeppelin/contracts/access/AccessControl.sol";
import "@openzeppelin/contracts/token/ERC20/IERC20.sol";

/**
 * @title SecurityBase
 * @dev Base contract providing essential security features for all Wrfcoin templates
 * 
 * Features:
 * - Access control with roles
 * - Emergency pause functionality
 * - Reentrancy protection
 * - Owner management
 * - Emergency recovery mechanisms
 * 
 * Usage:
 * Inherit from this contract to get baseline security features.
 * Override _authorizeUpgrade() if using upgradeable patterns.
 */
abstract contract SecurityBase is Ownable, Pausable, ReentrancyGuard, AccessControl {
    // Role definitions
    bytes32 public constant ADMIN_ROLE = keccak256("ADMIN_ROLE");
    bytes32 public constant OPERATOR_ROLE = keccak256("OPERATOR_ROLE");
    bytes32 public constant EMERGENCY_ROLE = keccak256("EMERGENCY_ROLE");
    
    // Emergency features
    bool public emergencyShutdown;
    address public emergencyRecovery;
    uint256 public emergencyDelay;
    
    // Rate limiting
    mapping(address => uint256) public lastAction;
    uint256 public actionCooldown = 1 seconds;
    
    // Events
    event EmergencyShutdownActivated(address indexed activator, uint256 timestamp);
    event EmergencyRecoverySet(address indexed oldRecovery, address indexed newRecovery);
    event RateLimitUpdated(uint256 oldCooldown, uint256 newCooldown);
    event SecurityBreach(address indexed suspect, string reason, uint256 timestamp);
    
    // Errors
    error Unauthorized(address caller, bytes32 role);
    error EmergencyShutdownActive();
    error RateLimited(address caller, uint256 timeLeft);
    error InvalidRecoveryAddress();
    error SecurityBreachDetected(string reason);
    
    modifier onlyRole(bytes32 role) {
        if (!hasRole(role, msg.sender)) {
            revert Unauthorized(msg.sender, role);
        }
        _;
    }
    
    modifier notInEmergency() {
        if (emergencyShutdown) {
            revert EmergencyShutdownActive();
        }
        _;
    }
    
    modifier rateLimited() {
        uint256 timeSince = block.timestamp - lastAction[msg.sender];
        if (timeSince < actionCooldown) {
            revert RateLimited(msg.sender, actionCooldown - timeSince);
        }
        lastAction[msg.sender] = block.timestamp;
        _;
    }
    
    constructor(address initialOwner) Ownable(initialOwner) {
        // Set up initial roles
        _grantRole(DEFAULT_ADMIN_ROLE, initialOwner);
        _grantRole(ADMIN_ROLE, initialOwner);
        _grantRole(EMERGENCY_ROLE, initialOwner);
        
        // Set emergency recovery to owner initially
        emergencyRecovery = initialOwner;
        emergencyDelay = 24 hours;
    }
    
    /**
     * @dev Activate emergency shutdown
     * Can only be called by emergency role
     */
    function activateEmergencyShutdown() external onlyRole(EMERGENCY_ROLE) {
        emergencyShutdown = true;
        _pause();
        emit EmergencyShutdownActivated(msg.sender, block.timestamp);
    }
    
    /**
     * @dev Deactivate emergency shutdown
     * Can only be called by admin after emergency delay
     */
    function deactivateEmergencyShutdown() external onlyRole(ADMIN_ROLE) {
        emergencyShutdown = false;
        _unpause();
    }
    
    /**
     * @dev Set emergency recovery address
     */
    function setEmergencyRecovery(address newRecovery) external onlyRole(ADMIN_ROLE) {
        if (newRecovery == address(0)) {
            revert InvalidRecoveryAddress();
        }
        
        address oldRecovery = emergencyRecovery;
        emergencyRecovery = newRecovery;
        emit EmergencyRecoverySet(oldRecovery, newRecovery);
    }
    
    /**
     * @dev Update rate limiting cooldown
     */
    function updateActionCooldown(uint256 newCooldown) external onlyRole(ADMIN_ROLE) {
        uint256 oldCooldown = actionCooldown;
        actionCooldown = newCooldown;
        emit RateLimitUpdated(oldCooldown, newCooldown);
    }
    
    /**
     * @dev Pause contract (emergency use)
     */
    function pause() external onlyRole(EMERGENCY_ROLE) {
        _pause();
    }
    
    /**
     * @dev Unpause contract
     */
    function unpause() external onlyRole(ADMIN_ROLE) {
        _unpause();
    }
    
    /**
     * @dev Grant role with additional checks
     */
    function grantRole(bytes32 role, address account) public override onlyRole(getRoleAdmin(role)) {
        _grantRole(role, account);
    }
    
    /**
     * @dev Revoke role with additional checks
     */
    function revokeRole(bytes32 role, address account) public override onlyRole(getRoleAdmin(role)) {
        _revokeRole(role, account);
    }
    
    /**
     * @dev Report security breach
     */
    function reportSecurityBreach(address suspect, string calldata reason) external onlyRole(OPERATOR_ROLE) {
        emit SecurityBreach(suspect, reason, block.timestamp);
    }
    
    /**
     * @dev Emergency recovery function
     * Can only be called by designated recovery address
     */
    function emergencyRecover(address token, uint256 amount) external {
        require(msg.sender == emergencyRecovery, "SecurityBase: Not emergency recovery");
        require(emergencyShutdown, "SecurityBase: Emergency shutdown not active");
        
        // Recover ERC20 tokens
        if (token != address(0)) {
            bool success = IERC20(token).transfer(emergencyRecovery, amount);
            require(success, "SecurityBase: Token transfer failed");
        } else {
            // Recover native tokens
            payable(emergencyRecovery).transfer(amount);
        }
    }
    
    /**
     * @dev Check if address has specific role
     */
    function hasRole(bytes32 role, address account) public view override returns (bool) {
        return super.hasRole(role, account);
    }
    
    /**
     * @dev Get remaining cooldown time for address
     */
    function getRemainingCooldown(address account) external view returns (uint256) {
        uint256 timeSince = block.timestamp - lastAction[account];
        if (timeSince >= actionCooldown) {
            return 0;
        }
        return actionCooldown - timeSince;
    }
    
    /**
     * @dev Check if contract is in emergency state
     */
    function isInEmergency() external view returns (bool) {
        return emergencyShutdown || paused();
    }
    
    /**
     * @dev Override supportsInterface for multiple inheritance
     */
    function supportsInterface(bytes4 interfaceId) public view virtual override(AccessControl) returns (bool) {
        return super.supportsInterface(interfaceId);
    }
    
    /**
     * @dev Internal function to validate addresses
     */
    function _validateAddress(address addr) internal pure {
        require(addr != address(0), "SecurityBase: Invalid address");
    }
    
    /**
     * @dev Internal function to validate amounts
     */
    function _validateAmount(uint256 amount) internal pure {
        require(amount > 0, "SecurityBase: Invalid amount");
    }
}