// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import "../base/SecurityBase.sol";
import "../nft/BaseERC721NFT.sol";

/**
 * @title CarbonMarketplace
 * @dev Comprehensive carbon credit trading marketplace
 * 
 * Features:
 * - Carbon credit NFT trading
 * - Verification and certification system
 * - Price discovery mechanisms
 * - Batch trading and bundling
 * - Retirement and offsetting tracking
 * - Quality scoring and ratings
 * - Automated market making
 * 
 * Use Cases:
 * - Carbon offset trading
 * - Corporate sustainability programs
 * - Individual carbon footprint offsetting
 * - Carbon credit investment
 * - Environmental impact certificates
 */
contract CarbonMarketplace is SecurityBase {
    enum CreditType {
        FOREST_CONSERVATION,
        RENEWABLE_ENERGY,
        METHANE_CAPTURE,
        DIRECT_AIR_CAPTURE,
        SOIL_CARBON,
        BLUE_CARBON,
        INDUSTRIAL_EFFICIENCY,
        WASTE_MANAGEMENT
    }
    
    enum CreditStandard {
        VCS,            // Verified Carbon Standard
        GOLD_STANDARD,  // Gold Standard
        CAR,            // Climate Action Reserve
        ACR,            // American Carbon Registry
        CDM,            // Clean Development Mechanism
        PLAN_VIVO,      // Plan Vivo
        CCBS,           // Climate, Community & Biodiversity Standards
        CUSTOM          // Project-specific standard
    }
    
    enum ListingType {
        FIXED_PRICE,    // Fixed price listing
        AUCTION,        // Auction-based listing
        BUNDLE,         // Bundle of multiple credits
        AMM             // Automated market maker
    }
    
    struct CarbonCredit {
        uint256 creditId;
        CreditType creditType;
        CreditStandard standard;
        address issuer;
        address currentOwner;
        uint256 carbonOffsetTons;  // CO2 equivalent in tons
        string projectName;
        string projectLocation;
        string verificationDocument;
        uint256 vintageYear;
        uint256 issuanceDate;
        uint256 expiryDate;
        bool isRetired;
        uint256 qualityScore;      // 1-100 quality rating
        uint256 pricePerTon;       // Current market price
        mapping(string => string) metadata; // Additional project data
    }
    
    struct MarketListing {
        uint256 listingId;
        uint256[] creditIds;
        address seller;
        ListingType listingType;
        uint256 pricePerTon;
        uint256 totalTons;
        uint256 minimumPurchase;
        uint256 expiryTime;
        bool isActive;
        bool isSold;
        uint256 createdAt;
    }
    
    struct AuctionData {
        uint256 startingPrice;
        uint256 currentBid;
        address highestBidder;
        uint256 bidIncrement;
        uint256 auctionEnd;
        bool isEnded;
        mapping(address => uint256) bidderDeposits;
    }
    
    struct QualityMetrics {
        uint256 additionality;     // 1-100
        uint256 permanence;        // 1-100
        uint256 measurability;     // 1-100
        uint256 verification;      // 1-100
        uint256 cobenefits;        // 1-100
        uint256 transparency;      // 1-100
    }
    
    struct RetirementCertificate {
        uint256 certificateId;
        address retirer;
        uint256[] retiredCreditIds;
        uint256 totalTonsRetired;
        string retirementReason;
        uint256 retirementDate;
        string beneficiaryName;
        bool isVerified;
    }
    
    // Carbon credit storage
    mapping(uint256 => CarbonCredit) public carbonCredits;
    mapping(uint256 => QualityMetrics) public qualityMetrics;
    uint256 public creditCount;
    
    // Marketplace listings
    mapping(uint256 => MarketListing) public listings;
    mapping(uint256 => AuctionData) public auctions;
    uint256 public listingCount;
    
    // Retirement tracking
    mapping(uint256 => RetirementCertificate) public retirementCertificates;
    mapping(address => uint256[]) public userRetirements;
    uint256 public retirementCount;
    
    // Market statistics
    mapping(CreditType => uint256) public typeVolumes;
    mapping(CreditStandard => uint256) public standardVolumes;
    uint256 public totalVolumeTraded;
    uint256 public totalTonsRetired;
    
    // Verification and certification
    mapping(address => bool) public verifiedIssuers;
    mapping(address => bool) public certifiedVerifiers;
    mapping(uint256 => address) public creditVerifiers;
    
    // Pricing and fees
    uint256 public marketplaceFeeRate = 250; // 2.5% in basis points
    uint256 public verificationFee = 0.1 ether;
    address public feeRecipient;
    
    // AMM (Automated Market Maker) for price discovery
    mapping(CreditType => uint256) public ammPools;
    mapping(CreditType => uint256) public ammPrices;
    uint256 public constant AMM_FEE_RATE = 30; // 0.3%
    
    // Events
    event CreditIssued(
        uint256 indexed creditId,
        CreditType creditType,
        address indexed issuer,
        uint256 carbonOffsetTons
    );
    event CreditListed(
        uint256 indexed listingId,
        uint256[] creditIds,
        address indexed seller,
        uint256 pricePerTon
    );
    event CreditSold(
        uint256 indexed listingId,
        address indexed buyer,
        address indexed seller,
        uint256 totalPrice
    );
    event CreditRetired(
        uint256 indexed certificateId,
        address indexed retirer,
        uint256[] creditIds,
        uint256 totalTons
    );
    event AuctionCreated(
        uint256 indexed listingId,
        uint256 startingPrice,
        uint256 auctionEnd
    );
    event BidPlaced(
        uint256 indexed listingId,
        address indexed bidder,
        uint256 bidAmount
    );
    event QualityScoreUpdated(
        uint256 indexed creditId,
        uint256 newScore,
        address indexed verifier
    );
    event IssuerVerified(address indexed issuer, bool verified);
    event VerifierCertified(address indexed verifier, bool certified);
    
    // Errors
    error CreditNotExists();
    error CreditAlreadyRetired();
    error NotCreditOwner();
    error ListingNotActive();
    error InsufficientFunds();
    error AuctionEnded();
    error AuctionNotEnded();
    error BidTooLow();
    error NotVerifiedIssuer();
    error NotCertifiedVerifier();
    error InvalidQualityScore();
    error ExceedsAvailableCredits();
    
    modifier creditExists(uint256 creditId) {
        if (creditId >= creditCount) {
            revert CreditNotExists();
        }
        _;
    }
    
    modifier onlyVerifiedIssuer() {
        if (!verifiedIssuers[msg.sender]) {
            revert NotVerifiedIssuer();
        }
        _;
    }
    
    modifier onlyCertifiedVerifier() {
        if (!certifiedVerifiers[msg.sender]) {
            revert NotCertifiedVerifier();
        }
        _;
    }
    
    constructor(address initialOwner) SecurityBase(initialOwner) {
        feeRecipient = initialOwner;
        verifiedIssuers[initialOwner] = true;
        certifiedVerifiers[initialOwner] = true;
    }
    
    /**
     * @dev Issue new carbon credits
     */
    function issueCredits(
        CreditType creditType,
        CreditStandard standard,
        uint256 carbonOffsetTons,
        string calldata projectName,
        string calldata projectLocation,
        string calldata verificationDocument,
        uint256 vintageYear,
        uint256 expiryDate,
        QualityMetrics calldata quality
    ) external onlyVerifiedIssuer returns (uint256) {
        uint256 creditId = creditCount++;
        
        // Validate quality metrics
        _validateQualityMetrics(quality);
        
        CarbonCredit storage credit = carbonCredits[creditId];
        credit.creditId = creditId;
        credit.creditType = creditType;
        credit.standard = standard;
        credit.issuer = msg.sender;
        credit.currentOwner = msg.sender;
        credit.carbonOffsetTons = carbonOffsetTons;
        credit.projectName = projectName;
        credit.projectLocation = projectLocation;
        credit.verificationDocument = verificationDocument;
        credit.vintageYear = vintageYear;
        credit.issuanceDate = block.timestamp;
        credit.expiryDate = expiryDate;
        credit.isRetired = false;
        credit.qualityScore = _calculateQualityScore(quality);
        credit.pricePerTon = ammPrices[creditType];
        
        qualityMetrics[creditId] = quality;
        
        emit CreditIssued(creditId, creditType, msg.sender, carbonOffsetTons);
        return creditId;
    }
    
    /**
     * @dev Create marketplace listing
     */
    function createListing(
        uint256[] calldata creditIds,
        ListingType listingType,
        uint256 pricePerTon,
        uint256 minimumPurchase,
        uint256 duration
    ) external returns (uint256) {
        // Validate credits ownership
        uint256 totalTons = 0;
        for (uint256 i = 0; i < creditIds.length; i++) {
            CarbonCredit storage credit = carbonCredits[creditIds[i]];
            require(credit.currentOwner == msg.sender, "CarbonMarketplace: Not credit owner");
            require(!credit.isRetired, "CarbonMarketplace: Credit already retired");
            totalTons += credit.carbonOffsetTons;
        }
        
        uint256 listingId = listingCount++;
        
        listings[listingId] = MarketListing({
            listingId: listingId,
            creditIds: creditIds,
            seller: msg.sender,
            listingType: listingType,
            pricePerTon: pricePerTon,
            totalTons: totalTons,
            minimumPurchase: minimumPurchase,
            expiryTime: block.timestamp + duration,
            isActive: true,
            isSold: false,
            createdAt: block.timestamp
        });
        
        // Handle auction setup
        if (listingType == ListingType.AUCTION) {
            _setupAuction(listingId, pricePerTon, duration);
        }
        
        emit CreditListed(listingId, creditIds, msg.sender, pricePerTon);
        return listingId;
    }
    
    /**
     * @dev Purchase carbon credits from listing
     */
    function purchaseCredits(
        uint256 listingId,
        uint256 tonsToPurchase
    ) external payable nonReentrant {
        MarketListing storage listing = listings[listingId];
        
        require(listing.isActive, "CarbonMarketplace: Listing not active");
        require(block.timestamp <= listing.expiryTime, "CarbonMarketplace: Listing expired");
        require(tonsToPurchase >= listing.minimumPurchase, "CarbonMarketplace: Below minimum purchase");
        require(tonsToPurchase <= listing.totalTons, "CarbonMarketplace: Exceeds available credits");
        
        uint256 totalPrice = tonsToPurchase * listing.pricePerTon;
        uint256 marketplaceFee = (totalPrice * marketplaceFeeRate) / 10000;
        uint256 sellerAmount = totalPrice - marketplaceFee;
        
        require(msg.value >= totalPrice, "CarbonMarketplace: Insufficient payment");
        
        // Transfer credits to buyer
        _transferCreditsFromListing(listingId, tonsToPurchase, msg.sender);
        
        // Process payments
        payable(listing.seller).transfer(sellerAmount);
        payable(feeRecipient).transfer(marketplaceFee);
        
        // Refund excess payment
        if (msg.value > totalPrice) {
            payable(msg.sender).transfer(msg.value - totalPrice);
        }
        
        // Update listing
        listing.totalTons -= tonsToPurchase;
        if (listing.totalTons == 0) {
            listing.isActive = false;
            listing.isSold = true;
        }
        
        // Update market statistics
        totalVolumeTraded += tonsToPurchase;
        
        emit CreditSold(listingId, msg.sender, listing.seller, totalPrice);
    }
    
    /**
     * @dev Place bid on auction
     */
    function placeBid(uint256 listingId) external payable {
        MarketListing storage listing = listings[listingId];
        AuctionData storage auction = auctions[listingId];
        
        require(listing.listingType == ListingType.AUCTION, "CarbonMarketplace: Not an auction");
        require(listing.isActive, "CarbonMarketplace: Auction not active");
        require(block.timestamp <= auction.auctionEnd, "CarbonMarketplace: Auction ended");
        
        uint256 requiredBid = auction.currentBid + auction.bidIncrement;
        require(msg.value >= requiredBid, "CarbonMarketplace: Bid too low");
        
        // Refund previous highest bidder
        if (auction.highestBidder != address(0)) {
            payable(auction.highestBidder).transfer(auction.currentBid);
        }
        
        auction.currentBid = msg.value;
        auction.highestBidder = msg.sender;
        
        emit BidPlaced(listingId, msg.sender, msg.value);
    }
    
    /**
     * @dev End auction and transfer credits
     */
    function endAuction(uint256 listingId) external {
        MarketListing storage listing = listings[listingId];
        AuctionData storage auction = auctions[listingId];
        
        require(listing.listingType == ListingType.AUCTION, "CarbonMarketplace: Not an auction");
        require(block.timestamp > auction.auctionEnd, "CarbonMarketplace: Auction still active");
        require(!auction.isEnded, "CarbonMarketplace: Auction already ended");
        
        auction.isEnded = true;
        listing.isActive = false;
        
        if (auction.highestBidder != address(0)) {
            // Transfer credits to winner
            _transferCreditsFromListing(listingId, listing.totalTons, auction.highestBidder);
            
            // Process payment
            uint256 marketplaceFee = (auction.currentBid * marketplaceFeeRate) / 10000;
            uint256 sellerAmount = auction.currentBid - marketplaceFee;
            
            payable(listing.seller).transfer(sellerAmount);
            payable(feeRecipient).transfer(marketplaceFee);
            
            listing.isSold = true;
            totalVolumeTraded += listing.totalTons;
            
            emit CreditSold(listingId, auction.highestBidder, listing.seller, auction.currentBid);
        }
    }
    
    /**
     * @dev Retire carbon credits for offsetting
     */
    function retireCredits(
        uint256[] calldata creditIds,
        string calldata retirementReason,
        string calldata beneficiaryName
    ) external returns (uint256) {
        uint256 totalTons = 0;
        
        // Validate ownership and retire credits
        for (uint256 i = 0; i < creditIds.length; i++) {
            CarbonCredit storage credit = carbonCredits[creditIds[i]];
            require(credit.currentOwner == msg.sender, "CarbonMarketplace: Not credit owner");
            require(!credit.isRetired, "CarbonMarketplace: Credit already retired");
            
            credit.isRetired = true;
            totalTons += credit.carbonOffsetTons;
        }
        
        uint256 certificateId = retirementCount++;
        
        retirementCertificates[certificateId] = RetirementCertificate({
            certificateId: certificateId,
            retirer: msg.sender,
            retiredCreditIds: creditIds,
            totalTonsRetired: totalTons,
            retirementReason: retirementReason,
            retirementDate: block.timestamp,
            beneficiaryName: beneficiaryName,
            isVerified: false
        });
        
        userRetirements[msg.sender].push(certificateId);
        totalTonsRetired += totalTons;
        
        emit CreditRetired(certificateId, msg.sender, creditIds, totalTons);
        return certificateId;
    }
    
    /**
     * @dev Update quality score (by certified verifiers)
     */
    function updateQualityScore(
        uint256 creditId,
        QualityMetrics calldata newMetrics
    ) external onlyCertifiedVerifier creditExists(creditId) {
        _validateQualityMetrics(newMetrics);
        
        qualityMetrics[creditId] = newMetrics;
        uint256 newScore = _calculateQualityScore(newMetrics);
        carbonCredits[creditId].qualityScore = newScore;
        creditVerifiers[creditId] = msg.sender;
        
        emit QualityScoreUpdated(creditId, newScore, msg.sender);
    }
    
    /**
     * @dev Verify issuer (admin function)
     */
    function verifyIssuer(address issuer, bool verified) external onlyRole(ADMIN_ROLE) {
        verifiedIssuers[issuer] = verified;
        emit IssuerVerified(issuer, verified);
    }
    
    /**
     * @dev Certify verifier (admin function)
     */
    function certifyVerifier(address verifier, bool certified) external onlyRole(ADMIN_ROLE) {
        certifiedVerifiers[verifier] = certified;
        emit VerifierCertified(verifier, certified);
    }
    
    /**
     * @dev Add liquidity to AMM pool
     */
    function addLiquidityToAMM(CreditType creditType) external payable {
        require(msg.value > 0, "CarbonMarketplace: No liquidity provided");
        
        ammPools[creditType] += msg.value;
        
        // Update AMM price based on liquidity (simplified price discovery)
        _updateAMMPrice(creditType);
    }
    
    /**
     * @dev Get market statistics
     */
    function getMarketStats() external view returns (
        uint256 totalCredits,
        uint256 totalVolume,
        uint256 totalRetired,
        uint256 activeListing,
        uint256 totalIssuers
    ) {
        uint256 activeListings = 0;
        for (uint256 i = 0; i < listingCount; i++) {
            if (listings[i].isActive) {
                activeListings++;
            }
        }
        
        return (
            creditCount,
            totalVolumeTraded,
            totalTonsRetired,
            activeListings,
            0 // Would need to track issuer count
        );
    }
    
    /**
     * @dev Get credit details
     */
    function getCreditDetails(uint256 creditId) external view creditExists(creditId) returns (
        CreditType creditType,
        CreditStandard standard,
        address issuer,
        address currentOwner,
        uint256 carbonOffsetTons,
        string memory projectName,
        uint256 qualityScore,
        bool isRetired
    ) {
        CarbonCredit storage credit = carbonCredits[creditId];
        
        return (
            credit.creditType,
            credit.standard,
            credit.issuer,
            credit.currentOwner,
            credit.carbonOffsetTons,
            credit.projectName,
            credit.qualityScore,
            credit.isRetired
        );
    }
    
    /**
     * @dev Get user's retirement certificates
     */
    function getUserRetirements(address user) external view returns (uint256[] memory) {
        return userRetirements[user];
    }
    
    // Internal functions
    
    function _setupAuction(
        uint256 listingId,
        uint256 startingPrice,
        uint256 duration
    ) internal {
        auctions[listingId] = AuctionData({
            startingPrice: startingPrice,
            currentBid: 0,
            highestBidder: address(0),
            bidIncrement: startingPrice / 20, // 5% increment
            auctionEnd: block.timestamp + duration,
            isEnded: false
        });
        
        emit AuctionCreated(listingId, startingPrice, block.timestamp + duration);
    }
    
    function _transferCreditsFromListing(
        uint256 listingId,
        uint256 tonsToPurchase,
        address buyer
    ) internal {
        MarketListing storage listing = listings[listingId];
        uint256 tonsRemaining = tonsToPurchase;
        
        for (uint256 i = 0; i < listing.creditIds.length && tonsRemaining > 0; i++) {
            uint256 creditId = listing.creditIds[i];
            CarbonCredit storage credit = carbonCredits[creditId];
            
            if (credit.carbonOffsetTons <= tonsRemaining) {
                // Transfer entire credit
                credit.currentOwner = buyer;
                tonsRemaining -= credit.carbonOffsetTons;
            } else {
                // Split credit (simplified - in production, would create new credit)
                // For now, transfer entire credit if it's the last one needed
                credit.currentOwner = buyer;
                tonsRemaining = 0;
            }
        }
    }
    
    function _validateQualityMetrics(QualityMetrics calldata metrics) internal pure {
        require(
            metrics.additionality <= 100 &&
            metrics.permanence <= 100 &&
            metrics.measurability <= 100 &&
            metrics.verification <= 100 &&
            metrics.cobenefits <= 100 &&
            metrics.transparency <= 100,
            "CarbonMarketplace: Invalid quality metrics"
        );
    }
    
    function _calculateQualityScore(QualityMetrics memory metrics) internal pure returns (uint256) {
        return (
            metrics.additionality +
            metrics.permanence +
            metrics.measurability +
            metrics.verification +
            metrics.cobenefits +
            metrics.transparency
        ) / 6;
    }
    
    function _updateAMMPrice(CreditType creditType) internal {
        // Simplified AMM pricing - in production, would use more sophisticated algorithms
        uint256 poolSize = ammPools[creditType];
        uint256 basePrice = 10 ether; // Base price per ton
        
        // Price increases with pool size (demand)
        ammPrices[creditType] = basePrice + (poolSize / 100);
    }
}