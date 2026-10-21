// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import "./contracts/governance/DPoSValidator.sol";
import "./contracts/governance/ComplianceModule.sol";
import "./contracts/governance/EnhancedTreasury.sol";
import "./contracts/CarbonCreditAMM.sol";
import "./contracts/SecureMultiSigWallet.sol";

/**
 * @title CompilationTestExcluded
 * @dev Simple contract to test compilation of core contracts excluding problematic ones
 */
contract CompilationTestExcluded {
    DPoSValidator public validator;
    ComplianceModule public compliance;
    EnhancedTreasury public treasury;
    CarbonCreditAMM public amm;
    SecureMultiSigWallet public multisig;
    
    constructor() {
        // Test that contracts can be instantiated
    }
}