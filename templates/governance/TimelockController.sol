// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import "../base/SecurityBase.sol";

/**
 * @title TimelockController
 * @dev Time-locked execution system for governance proposals
 * 
 * Features:
 * - Configurable time delays for different operation types
 * - Role-based proposal and execution permissions
 * - Batch operation support
 * - Cancellation mechanisms
 * - Emergency execution for critical operations
 * - Operation scheduling and queuing
 * 
 * Use Cases:
 * - Protocol upgrade delays
 * - Treasury operation safety
 * - Administrative action delays
 * - Multi-signature timelock combinations
 */
contract TimelockController is SecurityBase {
    enum OperationType {
        STANDARD,       // Standard operations
        CRITICAL,       // Critical system changes
        TREASURY,       // Treasury operations
        UPGRADE,        // Contract upgrades
        EMERGENCY,      // Emergency operations
        GOVERNANCE      // Governance parameter changes
    }
    
    enum OperationState {
        UNSET,          // Operation not scheduled
        PENDING,        // Operation scheduled but not ready
        READY,          // Operation ready for execution
        EXECUTED,       // Operation executed
        CANCELLED       // Operation cancelled
    }
    
    struct Operation {
        bytes32 id;
        address target;
        uint256 value;
        bytes data;
        bytes32 predecessor;
        bytes32 salt;
        uint256 scheduledTime;
        uint256 executionTime;
        OperationType operationType;
        OperationState state;
        address proposer;
        bool executed;
        bool cancelled;
    }
    
    struct BatchOperation {
        bytes32 batchId;
        bytes32[] operationIds;
        uint256 scheduledTime;
        uint256 executionTime;
        OperationType operationType;
        bool allExecuted;
        address proposer;
    }
    
    // Timelock delays for different operation types
    mapping(OperationType => uint256) public delays;
    
    // Operation storage
    mapping(bytes32 => Operation) public operations;
    mapping(bytes32 => BatchOperation) public batchOperations;
    
    // Role definitions
    bytes32 public constant PROPOSER_ROLE = keccak256("PROPOSER_ROLE");
    bytes32 public constant EXECUTOR_ROLE = keccak256("EXECUTOR_ROLE");
    bytes32 public constant CANCELLER_ROLE = keccak256("CANCELLER_ROLE");
    bytes32 public constant GUARDIAN_ROLE = keccak256("GUARDIAN_ROLE");
    
    // Emergency settings
    mapping(address => bool) public emergencyExecutors;
    bool public emergencyMode;
    uint256 public emergencyModeExpiry;
    uint256 public constant MAX_EMERGENCY_DURATION = 7 days;
    
    // Operation tracking
    bytes32[] public pendingOperations;
    bytes32[] public executedOperations;
    mapping(address => bytes32[]) public proposerOperations;
    
    // Minimum and maximum delays
    uint256 public constant MIN_DELAY = 1 hours;
    uint256 public constant MAX_DELAY = 365 days;
    
    // Events
    event OperationScheduled(
        bytes32 indexed id,
        address indexed target,
        uint256 value,
        bytes data,
        bytes32 predecessor,
        uint256 delay,
        OperationType operationType
    );
    event OperationExecuted(bytes32 indexed id, address indexed executor);
    event OperationCancelled(bytes32 indexed id, address indexed canceller);
    event BatchOperationScheduled(
        bytes32 indexed batchId,
        bytes32[] operationIds,
        uint256 delay,
        OperationType operationType
    );
    event BatchOperationExecuted(bytes32 indexed batchId, address indexed executor);
    event DelayUpdated(OperationType operationType, uint256 oldDelay, uint256 newDelay);
    event EmergencyModeEnabled(uint256 expiryTime);
    event EmergencyModeDisabled();
    event EmergencyExecutorAdded(address indexed executor);
    event EmergencyExecutorRemoved(address indexed executor);
    
    // Errors
    error InsufficientDelay();
    error OperationNotReady();
    error OperationAlreadyExecuted();
    error OperationAlreadyCancelled();
    error OperationNotScheduled();
    error PredecessorNotExecuted();
    error InvalidDelay();
    error EmergencyModeActive();
    error EmergencyModeNotActive();
    error NotEmergencyExecutor();
    error BatchNotComplete();
    error InvalidOperationType();
    
    modifier onlyProposer() {
        require(hasRole(PROPOSER_ROLE, msg.sender), "TimelockController: Must have proposer role");
        _;
    }
    
    modifier onlyExecutor() {
        require(hasRole(EXECUTOR_ROLE, msg.sender), "TimelockController: Must have executor role");
        _;
    }
    
    modifier onlyCanceller() {
        require(hasRole(CANCELLER_ROLE, msg.sender), "TimelockController: Must have canceller role");
        _;
    }
    
    modifier onlyGuardian() {
        require(hasRole(GUARDIAN_ROLE, msg.sender), "TimelockController: Must have guardian role");
        _;
    }
    
    modifier onlyEmergencyExecutor() {
        if (!emergencyExecutors[msg.sender]) {
            revert NotEmergencyExecutor();
        }
        _;
    }
    
    modifier whenNotEmergencyMode() {
        if (emergencyMode && block.timestamp < emergencyModeExpiry) {
            revert EmergencyModeActive();
        }
        _;
    }
    
    modifier whenEmergencyMode() {
        if (!emergencyMode || block.timestamp >= emergencyModeExpiry) {
            revert EmergencyModeNotActive();
        }
        _;
    }
    
    constructor(
        address initialOwner,
        address[] memory proposers,
        address[] memory executors,
        address[] memory cancellers
    ) SecurityBase(initialOwner) {
        // Set up default delays
        _setupDefaultDelays();
        
        // Grant roles
        for (uint256 i = 0; i < proposers.length; i++) {
            _grantRole(PROPOSER_ROLE, proposers[i]);
        }
        
        for (uint256 i = 0; i < executors.length; i++) {
            _grantRole(EXECUTOR_ROLE, executors[i]);
        }
        
        for (uint256 i = 0; i < cancellers.length; i++) {
            _grantRole(CANCELLER_ROLE, cancellers[i]);
        }
        
        // Grant guardian role to owner
        _grantRole(GUARDIAN_ROLE, initialOwner);
        
        // Allow this contract to execute operations on itself
        _grantRole(EXECUTOR_ROLE, address(this));
    }
    
    /**
     * @dev Schedule a single operation
     */
    function schedule(
        address target,
        uint256 value,
        bytes calldata data,
        bytes32 predecessor,
        bytes32 salt,
        OperationType operationType
    ) external onlyProposer whenNotEmergencyMode returns (bytes32) {
        bytes32 id = hashOperation(target, value, data, predecessor, salt);
        
        require(operations[id].state == OperationState.UNSET, "TimelockController: Operation already scheduled");
        
        uint256 delay = delays[operationType];
        if (delay < MIN_DELAY) {
            revert InsufficientDelay();
        }
        
        uint256 executionTime = block.timestamp + delay;
        
        operations[id] = Operation({
            id: id,
            target: target,
            value: value,
            data: data,
            predecessor: predecessor,
            salt: salt,
            scheduledTime: block.timestamp,
            executionTime: executionTime,
            operationType: operationType,
            state: OperationState.PENDING,
            proposer: msg.sender,
            executed: false,
            cancelled: false
        });
        
        pendingOperations.push(id);
        proposerOperations[msg.sender].push(id);
        
        emit OperationScheduled(id, target, value, data, predecessor, delay, operationType);
        return id;
    }
    
    /**
     * @dev Schedule multiple operations as a batch
     */
    function scheduleBatch(
        address[] calldata targets,
        uint256[] calldata values,
        bytes[] calldata payloads,
        bytes32 predecessor,
        bytes32 salt,
        OperationType operationType
    ) external onlyProposer whenNotEmergencyMode returns (bytes32) {
        require(targets.length == values.length && targets.length == payloads.length,
               "TimelockController: Length mismatch");
        require(targets.length > 0, "TimelockController: Empty batch");
        
        bytes32[] memory operationIds = new bytes32[](targets.length);
        
        // Schedule individual operations
        for (uint256 i = 0; i < targets.length; i++) {
            bytes32 id = hashOperation(targets[i], values[i], payloads[i], predecessor, salt);
            operationIds[i] = id;
            
            uint256 delay = delays[operationType];
            uint256 executionTime = block.timestamp + delay;
            
            operations[id] = Operation({
                id: id,
                target: targets[i],
                value: values[i],
                data: payloads[i],
                predecessor: predecessor,
                salt: salt,
                scheduledTime: block.timestamp,
                executionTime: executionTime,
                operationType: operationType,
                state: OperationState.PENDING,
                proposer: msg.sender,
                executed: false,
                cancelled: false
            });
            
            pendingOperations.push(id);
        }
        
        // Create batch operation
        bytes32 batchId = keccak256(abi.encode(operationIds, block.timestamp, msg.sender));
        batchOperations[batchId] = BatchOperation({
            batchId: batchId,
            operationIds: operationIds,
            scheduledTime: block.timestamp,
            executionTime: block.timestamp + delays[operationType],
            operationType: operationType,
            allExecuted: false,
            proposer: msg.sender
        });
        
        emit BatchOperationScheduled(batchId, operationIds, delays[operationType], operationType);
        return batchId;
    }
    
    /**
     * @dev Execute a scheduled operation
     */
    function execute(
        address target,
        uint256 value,
        bytes calldata data,
        bytes32 predecessor,
        bytes32 salt
    ) external payable onlyExecutor nonReentrant {
        bytes32 id = hashOperation(target, value, data, predecessor, salt);
        Operation storage operation = operations[id];
        
        _validateExecution(operation);
        
        // Check predecessor
        if (predecessor != bytes32(0)) {
            require(operations[predecessor].executed, "TimelockController: Predecessor not executed");
        }
        
        operation.executed = true;
        operation.state = OperationState.EXECUTED;
        
        // Remove from pending operations
        _removePendingOperation(id);
        executedOperations.push(id);
        
        // Execute the operation
        (bool success, ) = target.call{value: value}(data);
        require(success, "TimelockController: Execution failed");
        
        emit OperationExecuted(id, msg.sender);
    }
    
    /**
     * @dev Execute a batch of operations
     */
    function executeBatch(
        address[] calldata targets,
        uint256[] calldata values,
        bytes[] calldata payloads,
        bytes32 predecessor,
        bytes32 salt
    ) external payable onlyExecutor nonReentrant {
        require(targets.length == values.length && targets.length == payloads.length,
               "TimelockController: Length mismatch");
        
        // Execute all operations in the batch
        for (uint256 i = 0; i < targets.length; i++) {
            bytes32 id = hashOperation(targets[i], values[i], payloads[i], predecessor, salt);
            Operation storage operation = operations[id];
            
            _validateExecution(operation);
            
            operation.executed = true;
            operation.state = OperationState.EXECUTED;
            
            // Execute the operation
            (bool success, ) = targets[i].call{value: values[i]}(payloads[i]);
            require(success, "TimelockController: Batch execution failed");
            
            emit OperationExecuted(id, msg.sender);
        }
        
        // Mark batch as executed
        bytes32 batchId = keccak256(abi.encode(
            _hashOperationBatch(targets, values, payloads, predecessor, salt),
            block.timestamp,
            msg.sender
        ));
        
        if (batchOperations[batchId].batchId == batchId) {
            batchOperations[batchId].allExecuted = true;
            emit BatchOperationExecuted(batchId, msg.sender);
        }
    }
    
    /**
     * @dev Emergency execute without timelock delay
     */
    function emergencyExecute(
        address target,
        uint256 value,
        bytes calldata data
    ) external payable onlyEmergencyExecutor whenEmergencyMode nonReentrant {
        (bool success, ) = target.call{value: value}(data);
        require(success, "TimelockController: Emergency execution failed");
    }
    
    /**
     * @dev Cancel a scheduled operation
     */
    function cancel(bytes32 id) external onlyCanceller {
        Operation storage operation = operations[id];
        
        require(operation.state == OperationState.PENDING || operation.state == OperationState.READY,
               "TimelockController: Cannot cancel executed operation");
        
        operation.cancelled = true;
        operation.state = OperationState.CANCELLED;
        
        _removePendingOperation(id);
        
        emit OperationCancelled(id, msg.sender);
    }
    
    /**
     * @dev Enable emergency mode
     */
    function enableEmergencyMode(uint256 duration) external onlyGuardian {
        require(duration <= MAX_EMERGENCY_DURATION, "TimelockController: Duration too long");
        
        emergencyMode = true;
        emergencyModeExpiry = block.timestamp + duration;
        
        emit EmergencyModeEnabled(emergencyModeExpiry);
    }
    
    /**
     * @dev Disable emergency mode
     */
    function disableEmergencyMode() external onlyGuardian {
        emergencyMode = false;
        emergencyModeExpiry = 0;
        
        emit EmergencyModeDisabled();
    }
    
    /**
     * @dev Add emergency executor
     */
    function addEmergencyExecutor(address executor) external onlyRole(ADMIN_ROLE) {
        emergencyExecutors[executor] = true;
        emit EmergencyExecutorAdded(executor);
    }
    
    /**
     * @dev Remove emergency executor
     */
    function removeEmergencyExecutor(address executor) external onlyRole(ADMIN_ROLE) {
        emergencyExecutors[executor] = false;
        emit EmergencyExecutorRemoved(executor);
    }
    
    /**
     * @dev Update delay for operation type
     */
    function updateDelay(OperationType operationType, uint256 newDelay) 
        external 
        onlyRole(ADMIN_ROLE) 
    {
        if (newDelay < MIN_DELAY || newDelay > MAX_DELAY) {
            revert InvalidDelay();
        }
        
        uint256 oldDelay = delays[operationType];
        delays[operationType] = newDelay;
        
        emit DelayUpdated(operationType, oldDelay, newDelay);
    }
    
    /**
     * @dev Check if operation is ready for execution
     */
    function isOperationReady(bytes32 id) external view returns (bool) {
        Operation storage operation = operations[id];
        return operation.state == OperationState.PENDING && 
               block.timestamp >= operation.executionTime;
    }
    
    /**
     * @dev Get operation state
     */
    function getOperationState(bytes32 id) external view returns (OperationState) {
        Operation storage operation = operations[id];
        
        if (operation.state == OperationState.PENDING && 
            block.timestamp >= operation.executionTime) {
            return OperationState.READY;
        }
        
        return operation.state;
    }
    
    /**
     * @dev Get pending operations count
     */
    function getPendingOperationsCount() external view returns (uint256) {
        return pendingOperations.length;
    }
    
    /**
     * @dev Get operations by proposer
     */
    function getOperationsByProposer(address proposer) external view returns (bytes32[] memory) {
        return proposerOperations[proposer];
    }
    
    /**
     * @dev Hash operation parameters
     */
    function hashOperation(
        address target,
        uint256 value,
        bytes calldata data,
        bytes32 predecessor,
        bytes32 salt
    ) public pure returns (bytes32) {
        return keccak256(abi.encode(target, value, data, predecessor, salt));
    }
    
    /**
     * @dev Get minimum delay for operation type
     */
    function getMinDelay(OperationType operationType) external view returns (uint256) {
        return delays[operationType];
    }
    
    // Internal functions
    
    function _validateExecution(Operation storage operation) internal view {
        require(operation.state != OperationState.UNSET, "TimelockController: Operation not scheduled");
        require(!operation.executed, "TimelockController: Operation already executed");
        require(!operation.cancelled, "TimelockController: Operation cancelled");
        require(block.timestamp >= operation.executionTime, "TimelockController: Operation not ready");
    }
    
    function _removePendingOperation(bytes32 id) internal {
        for (uint256 i = 0; i < pendingOperations.length; i++) {
            if (pendingOperations[i] == id) {
                pendingOperations[i] = pendingOperations[pendingOperations.length - 1];
                pendingOperations.pop();
                break;
            }
        }
    }
    
    function _hashOperationBatch(
        address[] calldata targets,
        uint256[] calldata values,
        bytes[] calldata payloads,
        bytes32 predecessor,
        bytes32 salt
    ) internal pure returns (bytes32[] memory) {
        bytes32[] memory hashedOperations = new bytes32[](targets.length);
        for (uint256 i = 0; i < targets.length; i++) {
            hashedOperations[i] = keccak256(abi.encode(
                targets[i], values[i], payloads[i], predecessor, salt
            ));
        }
        return hashedOperations;
    }
    
    function _setupDefaultDelays() internal {
        delays[OperationType.STANDARD] = 2 days;
        delays[OperationType.CRITICAL] = 7 days;
        delays[OperationType.TREASURY] = 3 days;
        delays[OperationType.UPGRADE] = 14 days;
        delays[OperationType.EMERGENCY] = 1 hours;
        delays[OperationType.GOVERNANCE] = 5 days;
    }
    
    /**
     * @dev Receive ETH for operations that require value
     */
    receive() external payable {}
}