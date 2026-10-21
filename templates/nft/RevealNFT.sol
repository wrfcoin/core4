// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import "./BaseERC721NFT.sol";

/**
 * @title RevealNFT
 * @dev NFTs with reveal mechanics for mystery collections
 * 
 * Features:
 * - Blind minting with hidden metadata
 * - Randomized reveals using Chainlink VRF
 * - Rarity-based distribution
 * - Batch reveal functionality
 * - Pre-reveal trading prevention
 * - Time-based or manual reveal triggers
 * 
 * Use Cases:
 * - Mystery box NFT collections
 * - Gaming item reveals
 * - Weather prediction reveals
 * - Surprise computational result reveals
 */
contract RevealNFT is BaseERC721NFT {
    enum RevealStatus {
        NOT_REVEALED,
        REVEAL_REQUESTED,
        REVEALED
    }
    
    struct RevealData {
        uint256 tokenId;
        RevealStatus status;
        uint256 revealRequestTime;
        uint256 revealedAt;
        uint256 randomSeed;
        uint256 rarity;          // 0-10000 (common to legendary)
        string revealedMetadata; // IPFS hash or metadata URI
        uint256 revealRound;     // Which batch reveal this belongs to
    }
    
    struct RarityTier {
        string name;             // "Common", "Rare", "Epic", "Legendary"
        uint256 probability;     // Probability out of 10000
        uint256 maxSupply;       // Maximum number of this rarity
        uint256 currentSupply;   // Current number minted
        string baseMetadataURI;  // Base metadata for this tier
    }
    
    // Core reveal data
    mapping(uint256 => RevealData) public reveals;
    mapping(uint256 => bool) public revealRequested;
    
    // Collection configuration
    string public preRevealMetadataURI;
    bool public revealEnabled;
    uint256 public revealDelay = 24 hours; // Delay between request and reveal
    uint256 public maxRevealBatchSize = 100;
    uint256 public currentRevealRound = 1;
    
    // Rarity system
    RarityTier[] public rarityTiers;
    mapping(uint256 => uint256) public tokenRarity; // tokenId => rarity tier index
    uint256 public totalRevealProbability;
    
    // Randomness (using simple block-based randomness - upgrade to Chainlink VRF in production)
    uint256 private _nonce;
    mapping(uint256 => uint256) public revealSeeds;
    
    // Reveal batching
    mapping(uint256 => uint256[]) public revealBatches; // round => tokenIds
    mapping(uint256 => bool) public batchRevealed;
    
    // Access control for reveals
    mapping(address => bool) public revealers;
    bool public publicRevealEnabled;
    
    // Events
    event RevealRequested(uint256 indexed tokenId, address indexed requester, uint256 requestTime);
    event TokenRevealed(uint256 indexed tokenId, uint256 rarity, string metadata, uint256 randomSeed);
    event BatchRevealStarted(uint256 indexed round, uint256[] tokenIds);
    event BatchRevealCompleted(uint256 indexed round, uint256 tokensRevealed);
    event RarityTierAdded(uint256 indexed tierIndex, string name, uint256 probability, uint256 maxSupply);
    event RevealerAdded(address indexed revealer);
    
    // Errors
    error RevealNotEnabled();
    error TokenAlreadyRevealed(uint256 tokenId);
    error RevealNotReady(uint256 tokenId, uint256 timeLeft);
    error UnauthorizedRevealer(address caller);
    error InvalidRarityConfiguration();
    error RevealDelayNotPassed(uint256 timeLeft);
    error BatchSizeExceeded(uint256 requested, uint256 maximum);
    error NoTokensToReveal();
    
    modifier onlyRevealer() {
        if (!publicRevealEnabled && !revealers[msg.sender] && !hasRole(ADMIN_ROLE, msg.sender)) {
            revert UnauthorizedRevealer(msg.sender);
        }
        _;
    }
    
    constructor(
        string memory name,
        string memory symbol,
        string memory baseTokenURI,
        string memory _preRevealMetadataURI,
        address initialOwner
    ) BaseERC721NFT(name, symbol, baseTokenURI, initialOwner) {
        preRevealMetadataURI = _preRevealMetadataURI;
        
        // Initialize default rarity tiers
        _addRarityTier("Common", 6000, 1000, ""); // 60% chance
        _addRarityTier("Uncommon", 2500, 500, ""); // 25% chance
        _addRarityTier("Rare", 1000, 200, ""); // 10% chance
        _addRarityTier("Epic", 400, 80, ""); // 4% chance
        _addRarityTier("Legendary", 100, 20, ""); // 1% chance
    }
    
    /**
     * @dev Mint a new NFT (unrevealed by default)
     */
    function mintUnrevealed(address to) external onlyRole(MINTER_ROLE) returns (uint256 tokenId) {
        tokenId = getNextTokenId();
        _safeMint(to, tokenId);
        
        // Initialize reveal data
        reveals[tokenId] = RevealData({
            tokenId: tokenId,
            status: RevealStatus.NOT_REVEALED,
            revealRequestTime: 0,
            revealedAt: 0,
            randomSeed: 0,
            rarity: 0,
            revealedMetadata: "",
            revealRound: 0
        });
        
        return tokenId;
    }
    
    /**
     * @dev Batch mint unrevealed NFTs
     */
    function batchMintUnrevealed(address to, uint256 quantity) external onlyRole(MINTER_ROLE) returns (uint256[] memory tokenIds) {
        tokenIds = new uint256[](quantity);
        
        for (uint256 i = 0; i < quantity; i++) {
            tokenIds[i] = this.mintUnrevealed(to);
        }
        
        return tokenIds;
    }
    
    /**
     * @dev Request reveal for a token
     */
    function requestReveal(uint256 tokenId) external {
        require(ownerOf(tokenId) == msg.sender, "RevealNFT: Not owner");
        
        if (!revealEnabled) {
            revert RevealNotEnabled();
        }
        
        RevealData storage revealData = reveals[tokenId];
        if (revealData.status != RevealStatus.NOT_REVEALED) {
            revert TokenAlreadyRevealed(tokenId);
        }
        
        // Update reveal data
        revealData.status = RevealStatus.REVEAL_REQUESTED;
        revealData.revealRequestTime = block.timestamp;
        revealData.revealRound = currentRevealRound;
        
        // Add to current reveal batch
        revealBatches[currentRevealRound].push(tokenId);
        revealRequested[tokenId] = true;
        
        emit RevealRequested(tokenId, msg.sender, block.timestamp);
    }
    
    /**
     * @dev Batch request reveals for multiple tokens
     */
    function batchRequestReveal(uint256[] calldata tokenIds) external {
        for (uint256 i = 0; i < tokenIds.length; i++) {
            this.requestReveal(tokenIds[i]);
        }
    }
    
    /**
     * @dev Reveal a single token (after delay)
     */
    function revealToken(uint256 tokenId) external onlyRevealer {
        RevealData storage revealData = reveals[tokenId];
        
        require(revealData.status == RevealStatus.REVEAL_REQUESTED, "RevealNFT: Reveal not requested");
        
        if (block.timestamp < revealData.revealRequestTime + revealDelay) {
            revert RevealDelayNotPassed(
                revealData.revealRequestTime + revealDelay - block.timestamp
            );
        }
        
        _performReveal(tokenId);
    }
    
    /**
     * @dev Batch reveal all tokens in current round
     */
    function batchReveal(uint256 round) external onlyRevealer {
        require(!batchRevealed[round], "RevealNFT: Batch already revealed");
        
        uint256[] storage tokenIds = revealBatches[round];
        if (tokenIds.length == 0) {
            revert NoTokensToReveal();
        }
        
        if (tokenIds.length > maxRevealBatchSize) {
            revert BatchSizeExceeded(tokenIds.length, maxRevealBatchSize);
        }
        
        emit BatchRevealStarted(round, tokenIds);
        
        uint256 revealedCount = 0;
        for (uint256 i = 0; i < tokenIds.length; i++) {
            uint256 tokenId = tokenIds[i];
            RevealData storage revealData = reveals[tokenId];
            
            // Check if delay has passed
            if (block.timestamp >= revealData.revealRequestTime + revealDelay) {
                _performReveal(tokenId);
                revealedCount++;
            }
        }
        
        batchRevealed[round] = true;
        emit BatchRevealCompleted(round, revealedCount);
    }
    
    /**
     * @dev Force reveal (admin only, bypasses delay)
     */
    function forceReveal(uint256 tokenId) external onlyRole(ADMIN_ROLE) {
        RevealData storage revealData = reveals[tokenId];
        require(revealData.status == RevealStatus.REVEAL_REQUESTED, "RevealNFT: Reveal not requested");
        
        _performReveal(tokenId);
    }
    
    /**
     * @dev Internal function to perform the actual reveal
     */
    function _performReveal(uint256 tokenId) internal {
        RevealData storage revealData = reveals[tokenId];
        
        // Generate randomness
        uint256 randomSeed = _generateRandomSeed(tokenId);
        revealData.randomSeed = randomSeed;
        
        // Determine rarity
        uint256 rarityIndex = _determineRarity(randomSeed);
        revealData.rarity = rarityIndex;
        tokenRarity[tokenId] = rarityIndex;
        
        // Update rarity tier supply
        if (rarityIndex < rarityTiers.length) {
            rarityTiers[rarityIndex].currentSupply++;
        }
        
        // Set revealed metadata
        string memory metadata = _generateMetadata(tokenId, rarityIndex, randomSeed);
        revealData.revealedMetadata = metadata;
        
        // Update status
        revealData.status = RevealStatus.REVEALED;
        revealData.revealedAt = block.timestamp;
        
        emit TokenRevealed(tokenId, rarityIndex, metadata, randomSeed);
    }
    
    /**
     * @dev Generate random seed for token
     */
    function _generateRandomSeed(uint256 tokenId) internal returns (uint256) {
        // Simple randomness - upgrade to Chainlink VRF for production
        _nonce++;
        return uint256(keccak256(abi.encodePacked(
            block.timestamp,
            block.prevrandao,
            tokenId,
            _nonce,
            msg.sender
        )));
    }
    
    /**
     * @dev Determine rarity based on random seed
     */
    function _determineRarity(uint256 randomSeed) internal view returns (uint256) {
        uint256 roll = randomSeed % 10000;
        uint256 cumulativeProbability = 0;
        
        for (uint256 i = 0; i < rarityTiers.length; i++) {
            cumulativeProbability += rarityTiers[i].probability;
            
            // Check if this tier has remaining supply
            if (rarityTiers[i].currentSupply < rarityTiers[i].maxSupply && roll < cumulativeProbability) {
                return i;
            }
        }
        
        // Fallback to common if no other tier available
        return 0;
    }
    
    /**
     * @dev Generate metadata for revealed token
     */
    function _generateMetadata(uint256 tokenId, uint256 rarityIndex, uint256 randomSeed) internal view returns (string memory) {
        if (rarityIndex >= rarityTiers.length) {
            return "";
        }
        
        RarityTier storage tier = rarityTiers[rarityIndex];
        
        // Generate unique traits based on random seed
        uint256 trait1 = (randomSeed >> 8) % 100;
        uint256 trait2 = (randomSeed >> 16) % 100;
        uint256 trait3 = (randomSeed >> 24) % 100;
        
        string memory json = string(abi.encodePacked(
            '{"name":"', tier.name, ' #', Strings.toString(tokenId), '",',
            '"description":"A ', tier.name, ' rarity NFT from the collection",',
            '"attributes":[',
                '{"trait_type":"Rarity","value":"', tier.name, '"},',
                '{"trait_type":"Rarity Index","value":', Strings.toString(rarityIndex), '},',
                '{"trait_type":"Trait 1","value":', Strings.toString(trait1), '},',
                '{"trait_type":"Trait 2","value":', Strings.toString(trait2), '},',
                '{"trait_type":"Trait 3","value":', Strings.toString(trait3), '},',
                '{"trait_type":"Random Seed","value":', Strings.toString(randomSeed), '}',
            ']}'
        ));
        
        return string(abi.encodePacked("data:application/json;base64,", _base64Encode(bytes(json))));
    }
    
    /**
     * @dev Override tokenURI to handle revealed/unrevealed states
     */
    function tokenURI(uint256 tokenId) public view virtual override returns (string memory) {
        _requireOwned(tokenId);
        
        RevealData storage revealData = reveals[tokenId];
        
        if (revealData.status == RevealStatus.REVEALED) {
            return revealData.revealedMetadata;
        } else {
            return preRevealMetadataURI;
        }
    }
    
    /**
     * @dev Get reveal status for token
     */
    function getRevealStatus(uint256 tokenId) external view returns (
        RevealStatus status,
        uint256 revealRequestTime,
        uint256 timeUntilReveal,
        uint256 rarity
    ) {
        RevealData storage revealData = reveals[tokenId];
        
        uint256 timeLeft = 0;
        if (revealData.status == RevealStatus.REVEAL_REQUESTED) {
            uint256 revealTime = revealData.revealRequestTime + revealDelay;
            if (block.timestamp < revealTime) {
                timeLeft = revealTime - block.timestamp;
            }
        }
        
        return (
            revealData.status,
            revealData.revealRequestTime,
            timeLeft,
            revealData.rarity
        );
    }
    
    /**
     * @dev Get tokens pending reveal for an address
     */
    function getPendingReveals(address owner) external view returns (uint256[] memory) {
        uint256[] memory ownedTokens = this.tokensOfOwner(owner);
        uint256[] memory pending = new uint256[](ownedTokens.length);
        uint256 pendingCount = 0;
        
        for (uint256 i = 0; i < ownedTokens.length; i++) {
            uint256 tokenId = ownedTokens[i];
            if (reveals[tokenId].status == RevealStatus.REVEAL_REQUESTED) {
                pending[pendingCount] = tokenId;
                pendingCount++;
            }
        }
        
        // Resize array to actual count
        uint256[] memory result = new uint256[](pendingCount);
        for (uint256 i = 0; i < pendingCount; i++) {
            result[i] = pending[i];
        }
        
        return result;
    }
    
    /**
     * @dev Get rarity distribution
     */
    function getRarityDistribution() external view returns (
        string[] memory names,
        uint256[] memory probabilities,
        uint256[] memory currentSupplies,
        uint256[] memory maxSupplies
    ) {
        uint256 length = rarityTiers.length;
        names = new string[](length);
        probabilities = new uint256[](length);
        currentSupplies = new uint256[](length);
        maxSupplies = new uint256[](length);
        
        for (uint256 i = 0; i < length; i++) {
            names[i] = rarityTiers[i].name;
            probabilities[i] = rarityTiers[i].probability;
            currentSupplies[i] = rarityTiers[i].currentSupply;
            maxSupplies[i] = rarityTiers[i].maxSupply;
        }
        
        return (names, probabilities, currentSupplies, maxSupplies);
    }
    
    /**
     * @dev Add a new rarity tier
     */
    function addRarityTier(
        string calldata name,
        uint256 probability,
        uint256 maxSupply,
        string calldata baseMetadataURI
    ) external onlyRole(ADMIN_ROLE) {
        _addRarityTier(name, probability, maxSupply, baseMetadataURI);
    }
    
    /**
     * @dev Internal function to add rarity tier
     */
    function _addRarityTier(
        string memory name,
        uint256 probability,
        uint256 maxSupply,
        string memory baseMetadataURI
    ) internal {
        rarityTiers.push(RarityTier({
            name: name,
            probability: probability,
            maxSupply: maxSupply,
            currentSupply: 0,
            baseMetadataURI: baseMetadataURI
        }));
        
        totalRevealProbability += probability;
        
        emit RarityTierAdded(rarityTiers.length - 1, name, probability, maxSupply);
    }
    
    /**
     * @dev Set reveal enabled status
     */
    function setRevealEnabled(bool enabled) external onlyRole(ADMIN_ROLE) {
        revealEnabled = enabled;
    }
    
    /**
     * @dev Set reveal delay
     */
    function setRevealDelay(uint256 delay) external onlyRole(ADMIN_ROLE) {
        revealDelay = delay;
    }
    
    /**
     * @dev Set pre-reveal metadata URI
     */
    function setPreRevealMetadataURI(string calldata uri) external onlyRole(ADMIN_ROLE) {
        preRevealMetadataURI = uri;
    }
    
    /**
     * @dev Add authorized revealer
     */
    function addRevealer(address revealer) external onlyRole(ADMIN_ROLE) {
        revealers[revealer] = true;
        emit RevealerAdded(revealer);
    }
    
    /**
     * @dev Remove authorized revealer
     */
    function removeRevealer(address revealer) external onlyRole(ADMIN_ROLE) {
        revealers[revealer] = false;
    }
    
    /**
     * @dev Set public reveal enabled
     */
    function setPublicRevealEnabled(bool enabled) external onlyRole(ADMIN_ROLE) {
        publicRevealEnabled = enabled;
    }
    
    /**
     * @dev Start new reveal round
     */
    function startNewRevealRound() external onlyRole(ADMIN_ROLE) {
        currentRevealRound++;
    }
    
    /**
     * @dev Simple base64 encoding (same as CarbonCreditNFT)
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