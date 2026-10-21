// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import "./BaseERC721NFT.sol";

/**
 * @title CarbonCreditNFT
 * @dev Environmental impact certificates as NFTs
 * 
 * Features:
 * - Carbon credit tokenization
 * - Verification and certification tracking
 * - Retirement and offset mechanisms
 * - Quality scoring and metadata
 * - Marketplace integration
 * - Batch operations for efficiency
 * 
 * Use Cases:
 * - Carbon offset certificates
 * - Environmental project verification
 * - Renewable energy certificates
 * - Biodiversity credits
 */
contract CarbonCreditNFT is BaseERC721NFT {
    enum CreditType {
        CARBON_REMOVAL,     // Direct air capture, reforestation
        CARBON_AVOIDANCE,   // Renewable energy, efficiency
        BLUE_CARBON,        // Ocean and coastal ecosystems
        SOIL_CARBON,        // Agricultural and land use
        BIOCHAR,            // Biochar production and storage
        RENEWABLE_ENERGY    // Solar, wind, hydro certificates
    }
    
    enum CreditStatus {
        PENDING_VERIFICATION,
        VERIFIED,
        RETIRED,
        DISPUTED,
        CANCELLED
    }
    
    struct CarbonCredit {
        uint256 tokenId;
        CreditType creditType;
        CreditStatus status;
        
        // Quantification
        uint256 co2Equivalent;      // CO2 equivalent in grams
        uint256 creditVintage;      // Year of carbon action
        uint256 issuanceDate;       // When credit was issued
        uint256 expirationDate;     // When credit expires
        
        // Project information
        string projectId;           // Unique project identifier
        string methodology;         // Verification methodology used
        string location;            // Geographic location
        address projectDeveloper;   // Project developer address
        
        // Verification
        address[] verifiers;        // List of verifying parties
        uint256 verificationScore;  // Quality score (0-10000)
        string evidenceHash;        // IPFS hash of supporting evidence
        string certificationBody;   // Certification authority
        
        // Retirement tracking
        address retiredBy;          // Who retired the credit
        uint256 retirementDate;     // When credit was retired
        string retirementReason;    // Reason for retirement
        
        // Additional metadata
        mapping(string => string) additionalData; // Flexible metadata
    }
    
    // Core data
    mapping(uint256 => CarbonCredit) public carbonCredits;
    mapping(address => uint256[]) public userCredits;
    mapping(string => uint256[]) public projectCredits;
    
    // Verification system
    mapping(address => bool) public authorizedVerifiers;
    mapping(address => uint256) public verifierReputations;
    uint256 public minimumVerifications = 2;
    uint256 public verificationThreshold = 8000; // 80% approval needed
    
    // Retirement tracking
    mapping(address => uint256) public totalRetiredByUser;
    mapping(string => uint256) public totalRetiredByProject;
    uint256 public totalRetiredCredits;
    
    // Marketplace features
    mapping(uint256 => uint256) public creditPrices;
    mapping(uint256 => bool) public creditsForSale;
    uint256 public marketplaceFeeRate = 250; // 2.5%
    address public marketplaceFeeRecipient;
    
    // Events
    event CreditIssued(uint256 indexed tokenId, address indexed recipient, CreditType creditType, uint256 co2Equivalent);
    event CreditVerified(uint256 indexed tokenId, address indexed verifier, uint256 score);
    event CreditRetired(uint256 indexed tokenId, address indexed retiredBy, string reason);
    event CreditListed(uint256 indexed tokenId, uint256 price);
    event CreditSold(uint256 indexed tokenId, address indexed buyer, uint256 price);
    event VerifierAuthorized(address indexed verifier);
    event ProjectRegistered(string indexed projectId, address indexed developer);
    
    // Errors
    error CreditAlreadyRetired(uint256 tokenId);
    error CreditExpired(uint256 tokenId);
    error InsufficientVerifications(uint256 current, uint256 required);
    error UnauthorizedVerifier(address verifier);
    error CreditNotForSale(uint256 tokenId);
    error InsufficientCreditPayment(uint256 provided, uint256 required);
    error InvalidCreditType(uint8 creditType);
    
    constructor(
        string memory name,
        string memory symbol,
        string memory baseTokenURI,
        address initialOwner
    ) BaseERC721NFT(name, symbol, baseTokenURI, initialOwner) {
        marketplaceFeeRecipient = initialOwner;
    }
    
    /**
     * @dev Issue a new carbon credit NFT
     */
    function issueCredit(
        address recipient,
        CreditType creditType,
        uint256 co2Equivalent,
        uint256 creditVintage,
        uint256 expirationDate,
        string calldata projectId,
        string calldata methodology,
        string calldata location,
        address projectDeveloper,
        string calldata evidenceHash
    ) external onlyRole(MINTER_ROLE) returns (uint256 tokenId) {
        if (uint8(creditType) > 5) {
            revert InvalidCreditType(uint8(creditType));
        }
        
        tokenId = getNextTokenId();
        _safeMint(recipient, tokenId);
        
        // Initialize carbon credit data
        CarbonCredit storage credit = carbonCredits[tokenId];
        credit.tokenId = tokenId;
        credit.creditType = creditType;
        credit.status = CreditStatus.PENDING_VERIFICATION;
        credit.co2Equivalent = co2Equivalent;
        credit.creditVintage = creditVintage;
        credit.issuanceDate = block.timestamp;
        credit.expirationDate = expirationDate;
        credit.projectId = projectId;
        credit.methodology = methodology;
        credit.location = location;
        credit.projectDeveloper = projectDeveloper;
        credit.evidenceHash = evidenceHash;
        
        // Add to indexes
        userCredits[recipient].push(tokenId);
        projectCredits[projectId].push(tokenId);
        
        emit CreditIssued(tokenId, recipient, creditType, co2Equivalent);
        return tokenId;
    }
    
    /**
     * @dev Verify a carbon credit
     */
    function verifyCredit(uint256 tokenId, uint256 qualityScore, string calldata certificationBody) external {
        if (!authorizedVerifiers[msg.sender]) {
            revert UnauthorizedVerifier(msg.sender);
        }
        
        CarbonCredit storage credit = carbonCredits[tokenId];
        require(credit.status == CreditStatus.PENDING_VERIFICATION, "CarbonCreditNFT: Invalid status");
        require(qualityScore <= 10000, "CarbonCreditNFT: Invalid quality score");
        
        // Check if verifier already verified this credit
        for (uint i = 0; i < credit.verifiers.length; i++) {
            require(credit.verifiers[i] != msg.sender, "CarbonCreditNFT: Already verified");
        }
        
        // Add verification
        credit.verifiers.push(msg.sender);
        credit.verificationScore = ((credit.verificationScore * (credit.verifiers.length - 1)) + qualityScore) / credit.verifiers.length;
        
        if (bytes(certificationBody).length > 0) {
            credit.certificationBody = certificationBody;
        }
        
        // Check if credit meets verification requirements
        if (credit.verifiers.length >= minimumVerifications && 
            credit.verificationScore >= verificationThreshold) {
            credit.status = CreditStatus.VERIFIED;
        }
        
        // Update verifier reputation
        verifierReputations[msg.sender]++;
        
        emit CreditVerified(tokenId, msg.sender, qualityScore);
    }
    
    /**
     * @dev Retire a carbon credit (permanent offset)
     */
    function retireCredit(uint256 tokenId, string calldata reason) external {
        require(ownerOf(tokenId) == msg.sender, "CarbonCreditNFT: Not owner");
        
        CarbonCredit storage credit = carbonCredits[tokenId];
        require(credit.status == CreditStatus.VERIFIED, "CarbonCreditNFT: Not verified");
        require(block.timestamp <= credit.expirationDate, "CarbonCreditNFT: Credit expired");
        
        if (credit.status == CreditStatus.RETIRED) {
            revert CreditAlreadyRetired(tokenId);
        }
        
        // Retire the credit
        credit.status = CreditStatus.RETIRED;
        credit.retiredBy = msg.sender;
        credit.retirementDate = block.timestamp;
        credit.retirementReason = reason;
        
        // Update retirement tracking
        totalRetiredByUser[msg.sender] += credit.co2Equivalent;
        totalRetiredByProject[credit.projectId] += credit.co2Equivalent;
        totalRetiredCredits += credit.co2Equivalent;
        
        // Remove from sale if listed
        if (creditsForSale[tokenId]) {
            creditsForSale[tokenId] = false;
            creditPrices[tokenId] = 0;
        }
        
        emit CreditRetired(tokenId, msg.sender, reason);
    }
    
    /**
     * @dev List credit for sale on marketplace
     */
    function listCreditForSale(uint256 tokenId, uint256 price) external {
        require(ownerOf(tokenId) == msg.sender, "CarbonCreditNFT: Not owner");
        
        CarbonCredit storage credit = carbonCredits[tokenId];
        require(credit.status == CreditStatus.VERIFIED, "CarbonCreditNFT: Not verified");
        require(block.timestamp <= credit.expirationDate, "CarbonCreditNFT: Credit expired");
        
        creditPrices[tokenId] = price;
        creditsForSale[tokenId] = true;
        
        emit CreditListed(tokenId, price);
    }
    
    /**
     * @dev Purchase a credit from marketplace
     */
    function purchaseCredit(uint256 tokenId) external payable nonReentrant {
        if (!creditsForSale[tokenId]) {
            revert CreditNotForSale(tokenId);
        }
        
        uint256 price = creditPrices[tokenId];
        uint256 marketplaceFee = (price * marketplaceFeeRate) / 10000;
        uint256 sellerAmount = price - marketplaceFee;
        
        if (msg.value < price) {
            revert InsufficientCreditPayment(msg.value, price);
        }
        
        address seller = ownerOf(tokenId);
        
        // Transfer NFT
        _transfer(seller, msg.sender, tokenId);
        
        // Transfer payments
        payable(seller).transfer(sellerAmount);
        payable(marketplaceFeeRecipient).transfer(marketplaceFee);
        
        // Remove from sale
        creditsForSale[tokenId] = false;
        creditPrices[tokenId] = 0;
        
        // Refund excess payment
        if (msg.value > price) {
            payable(msg.sender).transfer(msg.value - price);
        }
        
        // Update user credits index
        userCredits[msg.sender].push(tokenId);
        
        emit CreditSold(tokenId, msg.sender, price);
    }
    
    /**
     * @dev Batch retire multiple credits
     */
    function batchRetireCredits(uint256[] calldata tokenIds, string calldata reason) external {
        for (uint i = 0; i < tokenIds.length; i++) {
            this.retireCredit(tokenIds[i], reason);
        }
    }
    
    /**
     * @dev Get credit details
     */
    function getCreditDetails(uint256 tokenId) external view returns (
        CreditType creditType,
        CreditStatus status,
        uint256 co2Equivalent,
        uint256 creditVintage,
        uint256 verificationScore,
        address retiredBy,
        uint256 retirementDate
    ) {
        CarbonCredit storage credit = carbonCredits[tokenId];
        return (
            credit.creditType,
            credit.status,
            credit.co2Equivalent,
            credit.creditVintage,
            credit.verificationScore,
            credit.retiredBy,
            credit.retirementDate
        );
    }
    
    /**
     * @dev Get credits by project
     */
    function getCreditsByProject(string calldata projectId) external view returns (uint256[] memory) {
        return projectCredits[projectId];
    }
    
    /**
     * @dev Get user's total carbon offset
     */
    function getUserCarbonOffset(address user) external view returns (uint256) {
        return totalRetiredByUser[user];
    }
    
    /**
     * @dev Get credits for sale with pagination
     */
    function getCreditsForSale(uint256 offset, uint256 limit) external view returns (
        uint256[] memory tokenIds,
        uint256[] memory prices,
        uint256[] memory co2Amounts
    ) {
        uint256 totalForSale = 0;
        uint256 currentSupply = getNextTokenId() - 1;
        
        // Count total for sale
        for (uint256 i = 1; i <= currentSupply; i++) {
            if (creditsForSale[i]) {
                totalForSale++;
            }
        }
        
        // Calculate actual limit
        uint256 actualLimit = limit;
        if (offset + limit > totalForSale) {
            actualLimit = totalForSale > offset ? totalForSale - offset : 0;
        }
        
        tokenIds = new uint256[](actualLimit);
        prices = new uint256[](actualLimit);
        co2Amounts = new uint256[](actualLimit);
        
        uint256 found = 0;
        uint256 skipped = 0;
        
        for (uint256 i = 1; i <= currentSupply && found < actualLimit; i++) {
            if (creditsForSale[i]) {
                if (skipped >= offset) {
                    tokenIds[found] = i;
                    prices[found] = creditPrices[i];
                    co2Amounts[found] = carbonCredits[i].co2Equivalent;
                    found++;
                }
                skipped++;
            }
        }
        
        return (tokenIds, prices, co2Amounts);
    }
    
    /**
     * @dev Add authorized verifier
     */
    function addVerifier(address verifier) external onlyRole(ADMIN_ROLE) {
        authorizedVerifiers[verifier] = true;
        emit VerifierAuthorized(verifier);
    }
    
    /**
     * @dev Remove authorized verifier
     */
    function removeVerifier(address verifier) external onlyRole(ADMIN_ROLE) {
        authorizedVerifiers[verifier] = false;
    }
    
    /**
     * @dev Set minimum verifications required
     */
    function setMinimumVerifications(uint256 minimum) external onlyRole(ADMIN_ROLE) {
        minimumVerifications = minimum;
    }
    
    /**
     * @dev Set verification threshold
     */
    function setVerificationThreshold(uint256 threshold) external onlyRole(ADMIN_ROLE) {
        require(threshold <= 10000, "CarbonCreditNFT: Invalid threshold");
        verificationThreshold = threshold;
    }
    
    /**
     * @dev Set marketplace fee rate
     */
    function setMarketplaceFeeRate(uint256 feeRate) external onlyRole(ADMIN_ROLE) {
        require(feeRate <= 1000, "CarbonCreditNFT: Fee too high"); // Max 10%
        marketplaceFeeRate = feeRate;
    }
    
    /**
     * @dev Set additional metadata for credit
     */
    function setAdditionalData(uint256 tokenId, string calldata key, string calldata value) external {
        require(ownerOf(tokenId) == msg.sender || hasRole(ADMIN_ROLE, msg.sender), "CarbonCreditNFT: Unauthorized");
        carbonCredits[tokenId].additionalData[key] = value;
    }
    
    /**
     * @dev Get additional metadata for credit
     */
    function getAdditionalData(uint256 tokenId, string calldata key) external view returns (string memory) {
        return carbonCredits[tokenId].additionalData[key];
    }
    
    /**
     * @dev Override tokenURI to include carbon credit metadata
     */
    function tokenURI(uint256 tokenId) public view virtual override returns (string memory) {
        _requireOwned(tokenId);
        
        CarbonCredit storage credit = carbonCredits[tokenId];
        
        // Create JSON metadata
        string memory json = string(abi.encodePacked(
            '{"name":"Carbon Credit #', Strings.toString(tokenId), '",',
            '"description":"Carbon credit representing ', Strings.toString(credit.co2Equivalent), ' grams CO2 equivalent",',
            '"attributes":[',
                '{"trait_type":"Credit Type","value":"', _getCreditTypeName(credit.creditType), '"},',
                '{"trait_type":"CO2 Equivalent","value":', Strings.toString(credit.co2Equivalent), '},',
                '{"trait_type":"Vintage","value":', Strings.toString(credit.creditVintage), '},',
                '{"trait_type":"Status","value":"', _getStatusName(credit.status), '"},',
                '{"trait_type":"Verification Score","value":', Strings.toString(credit.verificationScore), '}',
            ']}'
        ));
        
        return string(abi.encodePacked("data:application/json;base64,", _base64Encode(bytes(json))));
    }
    
    /**
     * @dev Get credit type name
     */
    function _getCreditTypeName(CreditType creditType) internal pure returns (string memory) {
        if (creditType == CreditType.CARBON_REMOVAL) return "Carbon Removal";
        if (creditType == CreditType.CARBON_AVOIDANCE) return "Carbon Avoidance";
        if (creditType == CreditType.BLUE_CARBON) return "Blue Carbon";
        if (creditType == CreditType.SOIL_CARBON) return "Soil Carbon";
        if (creditType == CreditType.BIOCHAR) return "Biochar";
        if (creditType == CreditType.RENEWABLE_ENERGY) return "Renewable Energy";
        return "Unknown";
    }
    
    /**
     * @dev Get status name
     */
    function _getStatusName(CreditStatus status) internal pure returns (string memory) {
        if (status == CreditStatus.PENDING_VERIFICATION) return "Pending";
        if (status == CreditStatus.VERIFIED) return "Verified";
        if (status == CreditStatus.RETIRED) return "Retired";
        if (status == CreditStatus.DISPUTED) return "Disputed";
        if (status == CreditStatus.CANCELLED) return "Cancelled";
        return "Unknown";
    }
    
    /**
     * @dev Simple base64 encoding
     */
    function _base64Encode(bytes memory data) internal pure returns (string memory) {
        if (data.length == 0) return "";
        
        string memory table = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        uint256 encodedLen = 4 * ((data.length + 2) / 3);
        string memory result = new string(encodedLen + 32);
        
        assembly {
            let tablePtr := add(table, 1)
            let resultPtr := add(result, 32)
            
            for {
                let i := 0
            } lt(i, mload(data)) {
                i := add(i, 3)
            } {
                let input := and(mload(add(add(data, i), 32)), 0xffffff)
                
                let out := mload(add(tablePtr, and(shr(18, input), 0x3F)))
                out := shl(8, out)
                out := add(out, and(mload(add(tablePtr, and(shr(12, input), 0x3F))), 0xFF))
                out := shl(8, out)
                out := add(out, and(mload(add(tablePtr, and(shr(6, input), 0x3F))), 0xFF))
                out := shl(8, out)
                out := add(out, and(mload(add(tablePtr, and(input, 0x3F))), 0xFF))
                out := shl(224, out)
                
                mstore(resultPtr, out)
                resultPtr := add(resultPtr, 4)
            }
            
            switch mod(mload(data), 3)
            case 1 {
                mstore(sub(resultPtr, 2), shl(240, 0x3d3d))
            }
            case 2 {
                mstore(sub(resultPtr, 1), shl(248, 0x3d))
            }
        }
        
        return result;
    }
}