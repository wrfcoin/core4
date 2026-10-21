// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import "@openzeppelin/contracts/token/ERC721/ERC721.sol";
import "@openzeppelin/contracts/token/ERC721/extensions/ERC721URIStorage.sol";
import "@openzeppelin/contracts/token/ERC721/extensions/ERC721Burnable.sol";
import "@openzeppelin/contracts/token/ERC721/extensions/ERC721Royalty.sol";
import "../base/SecurityBase.sol";

/**
 * @title BaseERC721NFT
 * @dev Comprehensive NFT template with advanced features
 * 
 * Features:
 * - ERC721 standard compliance
 * - URI storage for metadata
 * - Burnable tokens
 * - Royalty support (EIP-2981)
 * - Minting controls and caps
 * - Reveal mechanics
 * - Batch operations
 * - Transfer restrictions
 * - Marketplace integration
 * 
 * Use Cases:
 * - Art collections
 * - Gaming assets
 * - Membership tokens
 * - Certificates
 * - Utility NFTs
 */
contract BaseERC721NFT is 
    ERC721, 
    ERC721URIStorage, 
    ERC721Burnable, 
    ERC721Royalty,
    SecurityBase 
{
    // Token management
    uint256 private _nextTokenId = 1;
    uint256 public maxSupply;
    uint256 public mintPrice;
    uint256 public maxMintPerAddress;
    
    // Minting controls
    bool public mintingPaused;
    bool public publicMintEnabled;
    mapping(address => bool) public allowlist;
    mapping(address => uint256) public mintCount;
    
    // Reveal mechanics
    bool public revealed;
    string public baseTokenURI;
    string public hiddenMetadataURI;
    
    // Marketplace integration
    mapping(uint256 => uint256) public tokenPrices;
    mapping(uint256 => bool) public tokensForSale;
    mapping(address => bool) public approvedMarketplaces;
    
    // Royalty settings
    address public royaltyRecipient;
    uint96 public royaltyFraction; // in basis points (500 = 5%)
    
    // Batch operations
    uint256 public constant MAX_BATCH_SIZE = 20;
    
    // Roles
    bytes32 public constant MINTER_ROLE = keccak256("MINTER_ROLE");
    bytes32 public constant REVEALER_ROLE = keccak256("REVEALER_ROLE");
    
    // Events
    event BatchMinted(address indexed to, uint256 startTokenId, uint256 quantity);
    event Revealed(string newBaseURI);
    event TokenListed(uint256 indexed tokenId, uint256 price);
    event TokenSold(uint256 indexed tokenId, address from, address to, uint256 price);
    event RoyaltyUpdated(address recipient, uint96 fraction);
    event MintPriceUpdated(uint256 oldPrice, uint256 newPrice);
    
    // Errors
    error ExceedsMaxSupply();
    error ExceedsMaxMintPerAddress();
    error InsufficientPayment();
    error MintingPausedError();
    error PublicMintNotEnabled();
    error NotOnAllowlist();
    error TokenNotForSale();
    error InvalidPrice();
    error UnauthorizedMarketplace();
    
    modifier onlyTokenOwner(uint256 tokenId) {
        require(ownerOf(tokenId) == msg.sender, "BaseERC721NFT: Not token owner");
        _;
    }
    
    modifier mintCompliance(uint256 quantity) {
        if (mintingPaused) revert MintingPausedError();
        if (_nextTokenId + quantity - 1 > maxSupply) revert ExceedsMaxSupply();
        if (mintCount[msg.sender] + quantity > maxMintPerAddress) revert ExceedsMaxMintPerAddress();
        _;
    }
    
    constructor(
        string memory name,
        string memory symbol,
        uint256 _maxSupply,
        uint256 _mintPrice,
        uint256 _maxMintPerAddress,
        string memory _hiddenMetadataURI,
        address initialOwner
    ) ERC721(name, symbol) SecurityBase(initialOwner) {
        maxSupply = _maxSupply;
        mintPrice = _mintPrice;
        maxMintPerAddress = _maxMintPerAddress;
        hiddenMetadataURI = _hiddenMetadataURI;
        royaltyRecipient = initialOwner;
        royaltyFraction = 500; // 5% default royalty
        
        // Grant roles
        _grantRole(MINTER_ROLE, initialOwner);
        _grantRole(REVEALER_ROLE, initialOwner);
        
        // Set default royalty
        _setDefaultRoyalty(royaltyRecipient, royaltyFraction);
    }
    
    /**
     * @dev Public mint function
     */
    function mint(uint256 quantity) 
        external 
        payable 
        mintCompliance(quantity) 
        whenNotPaused 
    {
        if (!publicMintEnabled) revert PublicMintNotEnabled();
        if (msg.value < mintPrice * quantity) revert InsufficientPayment();
        
        _batchMint(msg.sender, quantity);
    }
    
    /**
     * @dev Allowlist mint function
     */
    function allowlistMint(uint256 quantity) 
        external 
        payable 
        mintCompliance(quantity) 
        whenNotPaused 
    {
        if (!allowlist[msg.sender]) revert NotOnAllowlist();
        if (msg.value < mintPrice * quantity) revert InsufficientPayment();
        
        _batchMint(msg.sender, quantity);
    }
    
    /**
     * @dev Admin mint function
     */
    function adminMint(address to, uint256 quantity) 
        external 
        onlyRole(MINTER_ROLE) 
        whenNotPaused 
    {
        if (_nextTokenId + quantity - 1 > maxSupply) revert ExceedsMaxSupply();
        _batchMint(to, quantity);
    }
    
    /**
     * @dev Batch mint implementation
     */
    function _batchMint(address to, uint256 quantity) internal {
        require(quantity <= MAX_BATCH_SIZE, "BaseERC721NFT: Batch size too large");
        
        uint256 startTokenId = _nextTokenId;
        
        for (uint256 i = 0; i < quantity; i++) {
            uint256 tokenId = _nextTokenId;
            ++_nextTokenId;
            _safeMint(to, tokenId);
        }
        
        mintCount[to] += quantity;
        emit BatchMinted(to, startTokenId, quantity);
    }
    
    /**
     * @dev Set base URI for revealed metadata
     */
    function reveal(string calldata newBaseURI) 
        external 
        onlyRole(REVEALER_ROLE) 
    {
        revealed = true;
        baseTokenURI = newBaseURI;
        emit Revealed(newBaseURI);
    }
    
    /**
     * @dev Set hidden metadata URI
     */
    function setHiddenMetadataURI(string calldata uri) 
        external 
        onlyRole(ADMIN_ROLE) 
    {
        hiddenMetadataURI = uri;
    }
    
    /**
     * @dev Update mint price
     */
    function setMintPrice(uint256 newPrice) 
        external 
        onlyRole(ADMIN_ROLE) 
    {
        uint256 oldPrice = mintPrice;
        mintPrice = newPrice;
        emit MintPriceUpdated(oldPrice, newPrice);
    }
    
    /**
     * @dev Toggle public minting
     */
    function setPublicMintEnabled(bool enabled) 
        external 
        onlyRole(ADMIN_ROLE) 
    {
        publicMintEnabled = enabled;
    }
    
    /**
     * @dev Toggle minting pause
     */
    function setMintingPaused(bool paused) 
        external 
        onlyRole(ADMIN_ROLE) 
    {
        mintingPaused = paused;
    }
    
    /**
     * @dev Add addresses to allowlist
     */
    function addToAllowlist(address[] calldata addresses) 
        external 
        onlyRole(ADMIN_ROLE) 
    {
        for (uint256 i = 0; i < addresses.length; i++) {
            allowlist[addresses[i]] = true;
        }
    }
    
    /**
     * @dev Remove addresses from allowlist
     */
    function removeFromAllowlist(address[] calldata addresses) 
        external 
        onlyRole(ADMIN_ROLE) 
    {
        for (uint256 i = 0; i < addresses.length; i++) {
            allowlist[addresses[i]] = false;
        }
    }
    
    /**
     * @dev List token for sale
     */
    function listTokenForSale(uint256 tokenId, uint256 price) 
        external 
        onlyTokenOwner(tokenId) 
    {
        if (price == 0) revert InvalidPrice();
        
        tokenPrices[tokenId] = price;
        tokensForSale[tokenId] = true;
        
        emit TokenListed(tokenId, price);
    }
    
    /**
     * @dev Remove token from sale
     */
    function removeTokenFromSale(uint256 tokenId) 
        external 
        onlyTokenOwner(tokenId) 
    {
        tokensForSale[tokenId] = false;
        delete tokenPrices[tokenId];
    }
    
    /**
     * @dev Purchase token
     */
    function purchaseToken(uint256 tokenId) 
        external 
        payable 
        nonReentrant 
    {
        if (!tokensForSale[tokenId]) revert TokenNotForSale();
        
        uint256 price = tokenPrices[tokenId];
        if (msg.value < price) revert InsufficientPayment();
        
        address seller = ownerOf(tokenId);
        
        // Remove from sale
        tokensForSale[tokenId] = false;
        delete tokenPrices[tokenId];
        
        // Calculate royalty
        (address royaltyReceiver, uint256 royaltyAmount) = royaltyInfo(tokenId, price);
        uint256 sellerAmount = price - royaltyAmount;
        
        // Transfer payments
        if (royaltyAmount > 0) {
            payable(royaltyReceiver).transfer(royaltyAmount);
        }
        payable(seller).transfer(sellerAmount);
        
        // Transfer token
        _transfer(seller, msg.sender, tokenId);
        
        // Refund excess payment
        if (msg.value > price) {
            payable(msg.sender).transfer(msg.value - price);
        }
        
        emit TokenSold(tokenId, seller, msg.sender, price);
    }
    
    /**
     * @dev Update royalty settings
     */
    function setRoyalty(address recipient, uint96 fraction) 
        external 
        onlyRole(ADMIN_ROLE) 
    {
        require(fraction <= 1000, "BaseERC721NFT: Royalty too high"); // Max 10%
        
        royaltyRecipient = recipient;
        royaltyFraction = fraction;
        _setDefaultRoyalty(recipient, fraction);
        
        emit RoyaltyUpdated(recipient, fraction);
    }
    
    /**
     * @dev Approve marketplace for trading
     */
    function setApprovedMarketplace(address marketplace, bool approved) 
        external 
        onlyRole(ADMIN_ROLE) 
    {
        approvedMarketplaces[marketplace] = approved;
    }
    
    /**
     * @dev Withdraw contract balance
     */
    function withdraw() external onlyRole(ADMIN_ROLE) {
        uint256 balance = address(this).balance;
        require(balance > 0, "BaseERC721NFT: No balance to withdraw");
        
        payable(owner()).transfer(balance);
    }
    
    /**
     * @dev Get token URI with reveal logic
     */
    function tokenURI(uint256 tokenId) 
        public 
        view 
        override(ERC721, ERC721URIStorage) 
        returns (string memory) 
    {
        require(_ownerOf(tokenId) != address(0), "BaseERC721NFT: URI query for nonexistent token");
        
        if (!revealed) {
            return hiddenMetadataURI;
        }
        
        string memory customURI = super.tokenURI(tokenId);
        if (bytes(customURI).length > 0) {
            return customURI;
        }
        
        return string(abi.encodePacked(baseTokenURI, Strings.toString(tokenId), ".json"));
    }
    
    /**
     * @dev Get contract information
     */
    function getContractInfo() external view returns (
        uint256 totalSupply,
        uint256 maxSupplyLimit,
        uint256 currentMintPrice,
        bool isRevealed,
        bool isPublicMintActive,
        bool isMintingPaused
    ) {
        return (
            _nextTokenId - 1,
            maxSupply,
            mintPrice,
            revealed,
            publicMintEnabled,
            mintingPaused
        );
    }
    
    /**
     * @dev Get tokens owned by address
     */
    function tokensOfOwner(address owner) 
        external 
        view 
        returns (uint256[] memory) 
    {
        uint256 tokenCount = balanceOf(owner);
        uint256[] memory tokens = new uint256[](tokenCount);
        uint256 tokenIndex = 0;
        
        for (uint256 i = 1; i < _nextTokenId; i++) {
            if (_ownerOf(i) != address(0) && ownerOf(i) == owner) {
                tokens[tokenIndex] = i;
                tokenIndex++;
            }
        }
        
        return tokens;
    }
    
    /**
     * @dev Check if token exists
     */
    function exists(uint256 tokenId) external view returns (bool) {
        return _ownerOf(tokenId) != address(0);
    }
    
    /**
     * @dev Get next token ID
     */
    function getNextTokenId() public view returns (uint256) {
        return _nextTokenId;
    }
    
    /**
     * @dev Override _beforeTokenTransfer for additional checks
     */
    function _beforeTokenTransfer(
        address from,
        address to,
        uint256 tokenId,
        uint256 batchSize
    ) internal override whenNotPaused {
        super._beforeTokenTransfer(from, to, tokenId, batchSize);
        
        // Remove from sale if being transferred
        if (from != address(0) && tokensForSale[tokenId]) {
            tokensForSale[tokenId] = false;
            delete tokenPrices[tokenId];
        }
    }
    
    /**
     * @dev Required override for multiple inheritance
     */
    function _burn(uint256 tokenId) 
        internal 
        override(ERC721, ERC721URIStorage, ERC721Royalty) 
    {
        super._burn(tokenId);
    }
    
    /**
     * @dev Required override for multiple inheritance
     */
    function supportsInterface(bytes4 interfaceId) 
        public 
        view 
        override(ERC721, ERC721Royalty, AccessControl) 
        returns (bool) 
    {
        return super.supportsInterface(interfaceId);
    }
}
