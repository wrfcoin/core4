// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import "../base/SecurityBase.sol";
import "../nft/BaseERC721NFT.sol";

/**
 * @title EnvironmentalImpact
 * @dev Comprehensive environmental impact tracking and verification system
 * 
 * Features:
 * - Environmental project registration and tracking
 * - Impact measurement and verification
 * - Milestone-based funding and rewards
 * - Stakeholder reporting and transparency
 * - Biodiversity and ecosystem monitoring
 * - Community impact assessment
 * - Long-term sustainability tracking
 * 
 * Use Cases:
 * - Reforestation projects
 * - Conservation initiatives
 * - Renewable energy projects
 * - Waste reduction programs
 * - Biodiversity preservation
 * - Community development
 */
contract EnvironmentalImpact is SecurityBase {
    enum ProjectType {
        REFORESTATION,
        CONSERVATION,
        RENEWABLE_ENERGY,
        WASTE_MANAGEMENT,
        WATER_CONSERVATION,
        BIODIVERSITY,
        SOIL_RESTORATION,
        OCEAN_CLEANUP,
        COMMUNITY_DEVELOPMENT,
        RESEARCH
    }
    
    enum ProjectStatus {
        PROPOSED,
        APPROVED,
        ACTIVE,
        COMPLETED,
        SUSPENDED,
        CANCELLED
    }
    
    enum MilestoneStatus {
        PENDING,
        IN_PROGRESS,
        COMPLETED,
        VERIFIED,
        REJECTED
    }
    
    struct EnvironmentalProject {
        uint256 projectId;
        ProjectType projectType;
        ProjectStatus status;
        address projectOwner;
        string name;
        string description;
        string location;
        uint256 startDate;
        uint256 endDate;
        uint256 targetBudget;
        uint256 raisedFunds;
        uint256 totalImpactScore;
        mapping(string => uint256) impactMetrics;
        uint256[] milestoneIds;
        bool isVerified;
        address verifier;
    }
    
    struct ImpactMilestone {
        uint256 milestoneId;
        uint256 projectId;
        string description;
        string targetMetric;
        uint256 targetValue;
        uint256 actualValue;
        uint256 deadline;
        uint256 fundingAmount;
        MilestoneStatus status;
        string verificationDocument;
        address verifier;
        uint256 verificationDate;
        mapping(string => uint256) measurements;
    }
    
    struct ImpactCertificate {
        uint256 certificateId;
        uint256 projectId;
        address recipient;
        string impactType;
        uint256 impactValue;
        string unit;
        uint256 issuanceDate;
        uint256 validityPeriod;
        bool isActive;
        string metadataURI;
    }
    
    struct BiodiversityMetrics {
        uint256 speciesCount;
        uint256 habitatArea;         // in hectares
        uint256 ecosystemHealth;     // 1-100 score
        uint256 connectivityIndex;   // 1-100 score
        uint256 threatLevel;         // 1-100 score (100 = high threat)
        uint256 conservationValue;   // 1-100 score
    }
    
    struct CommunityImpact {
        uint256 beneficiaryCount;
        uint256 jobsCreated;
        uint256 incomeGenerated;     // in wei
        uint256 capacityBuilding;    // 1-100 score
        uint256 healthImprovement;   // 1-100 score
        uint256 educationAccess;     // 1-100 score
        uint256 genderEquity;        // 1-100 score
    }
    
    struct StakeholderReport {
        uint256 reportId;
        uint256 projectId;
        address reporter;
        string reportType;           // "progress", "issue", "completion"
        string description;
        string evidenceURI;
        uint256 timestamp;
        bool isVerified;
        address verifier;
    }
    
    // Project management
    mapping(uint256 => EnvironmentalProject) public projects;
    mapping(uint256 => ImpactMilestone) public milestones;
    mapping(uint256 => ImpactCertificate) public certificates;
    uint256 public projectCount;
    uint256 public milestoneCount;
    uint256 public certificateCount;
    
    // Impact tracking
    mapping(uint256 => BiodiversityMetrics) public biodiversityData;
    mapping(uint256 => CommunityImpact) public communityImpacts;
    mapping(uint256 => StakeholderReport[]) public projectReports;
    
    // Verification system
    mapping(address => bool) public verifiedVerifiers;
    mapping(address => string) public verifierCredentials;
    mapping(uint256 => address[]) public projectVerifiers;
    
    // Funding and rewards
    mapping(uint256 => mapping(address => uint256)) public projectContributions;
    mapping(address => uint256) public stakeholderRewards;
    uint256 public rewardPool;
    
    // Global impact statistics
    mapping(ProjectType => uint256) public typeImpactScores;
    mapping(string => uint256) public globalMetrics;
    uint256 public totalProjectsCompleted;
    uint256 public totalFundsDeployed;
    
    // Events
    event ProjectRegistered(
        uint256 indexed projectId,
        ProjectType projectType,
        address indexed owner,
        string name
    );
    event MilestoneCreated(
        uint256 indexed milestoneId,
        uint256 indexed projectId,
        string targetMetric,
        uint256 targetValue
    );
    event MilestoneCompleted(
        uint256 indexed milestoneId,
        uint256 actualValue,
        address indexed verifier
    );
    event ImpactCertificateIssued(
        uint256 indexed certificateId,
        uint256 indexed projectId,
        address indexed recipient,
        uint256 impactValue
    );
    event FundingReceived(
        uint256 indexed projectId,
        address indexed contributor,
        uint256 amount
    );
    event RewardDistributed(
        address indexed recipient,
        uint256 amount,
        string reason
    );
    event VerifierCertified(
        address indexed verifier,
        string credentials
    );
    event StakeholderReportSubmitted(
        uint256 indexed reportId,
        uint256 indexed projectId,
        address indexed reporter
    );
    
    // Errors
    error ProjectNotExists();
    error NotProjectOwner();
    error ProjectNotActive();
    error MilestoneNotExists();
    error NotVerifiedVerifier();
    error InsufficientFunding();
    error MilestoneAlreadyCompleted();
    error InvalidImpactValue();
    error CertificateExpired();
    
    modifier projectExists(uint256 projectId) {
        if (projectId >= projectCount) {
            revert ProjectNotExists();
        }
        _;
    }
    
    modifier onlyProjectOwner(uint256 projectId) {
        if (projects[projectId].projectOwner != msg.sender) {
            revert NotProjectOwner();
        }
        _;
    }
    
    modifier onlyVerifiedVerifier() {
        if (!verifiedVerifiers[msg.sender]) {
            revert NotVerifiedVerifier();
        }
        _;
    }
    
    constructor(address initialOwner) SecurityBase(initialOwner) {
        // Set initial verifier
        verifiedVerifiers[initialOwner] = true;
        verifierCredentials[initialOwner] = "Initial System Verifier";
    }
    
    /**
     * @dev Receive ETH for project funding
     */
    receive() external payable {
        rewardPool += msg.value;
    }
    
    /**
     * @dev Register new environmental project
     */
    function registerProject(
        ProjectType projectType,
        string calldata name,
        string calldata description,
        string calldata location,
        uint256 duration,
        uint256 targetBudget,
        string[] calldata initialMetrics,
        uint256[] calldata initialValues
    ) external returns (uint256) {
        require(initialMetrics.length == initialValues.length, "EnvironmentalImpact: Arrays length mismatch");
        
        uint256 projectId = projectCount++;
        
        EnvironmentalProject storage project = projects[projectId];
        project.projectId = projectId;
        project.projectType = projectType;
        project.status = ProjectStatus.PROPOSED;
        project.projectOwner = msg.sender;
        project.name = name;
        project.description = description;
        project.location = location;
        project.startDate = block.timestamp;
        project.endDate = block.timestamp + duration;
        project.targetBudget = targetBudget;
        project.raisedFunds = 0;
        project.totalImpactScore = 0;
        project.isVerified = false;
        
        // Set initial impact metrics
        for (uint256 i = 0; i < initialMetrics.length; i++) {
            project.impactMetrics[initialMetrics[i]] = initialValues[i];
        }
        
        emit ProjectRegistered(projectId, projectType, msg.sender, name);
        return projectId;
    }
    
    /**
     * @dev Create milestone for project
     */
    function createMilestone(
        uint256 projectId,
        string calldata description,
        string calldata targetMetric,
        uint256 targetValue,
        uint256 deadline,
        uint256 fundingAmount
    ) external projectExists(projectId) onlyProjectOwner(projectId) returns (uint256) {
        uint256 milestoneId = milestoneCount++;
        
        ImpactMilestone storage milestone = milestones[milestoneId];
        milestone.milestoneId = milestoneId;
        milestone.projectId = projectId;
        milestone.description = description;
        milestone.targetMetric = targetMetric;
        milestone.targetValue = targetValue;
        milestone.deadline = deadline;
        milestone.fundingAmount = fundingAmount;
        milestone.status = MilestoneStatus.PENDING;
        
        projects[projectId].milestoneIds.push(milestoneId);
        
        emit MilestoneCreated(milestoneId, projectId, targetMetric, targetValue);
        return milestoneId;
    }
    
    /**
     * @dev Fund environmental project
     */
    function fundProject(uint256 projectId) external payable projectExists(projectId) {
        require(msg.value > 0, "EnvironmentalImpact: No funding provided");
        
        EnvironmentalProject storage project = projects[projectId];
        require(project.status == ProjectStatus.APPROVED || project.status == ProjectStatus.ACTIVE, "EnvironmentalImpact: Project not accepting funds");
        
        project.raisedFunds += msg.value;
        projectContributions[projectId][msg.sender] += msg.value;
        totalFundsDeployed += msg.value;
        
        // Activate project if funding threshold met
        if (project.status == ProjectStatus.APPROVED && project.raisedFunds >= project.targetBudget / 4) {
            project.status = ProjectStatus.ACTIVE;
        }
        
        emit FundingReceived(projectId, msg.sender, msg.value);
    }
    
    /**
     * @dev Complete milestone with impact measurements
     */
    function completeMilestone(
        uint256 milestoneId,
        uint256 actualValue,
        string calldata verificationDocument,
        string[] calldata measurementKeys,
        uint256[] calldata measurementValues
    ) external onlyVerifiedVerifier {
        require(milestoneId < milestoneCount, "EnvironmentalImpact: Milestone not exists");
        require(measurementKeys.length == measurementValues.length, "EnvironmentalImpact: Arrays length mismatch");
        
        ImpactMilestone storage milestone = milestones[milestoneId];
        require(milestone.status == MilestoneStatus.IN_PROGRESS || milestone.status == MilestoneStatus.PENDING, "EnvironmentalImpact: Milestone not active");
        
        milestone.actualValue = actualValue;
        milestone.status = MilestoneStatus.COMPLETED;
        milestone.verificationDocument = verificationDocument;
        milestone.verifier = msg.sender;
        milestone.verificationDate = block.timestamp;
        
        // Record measurements
        for (uint256 i = 0; i < measurementKeys.length; i++) {
            milestone.measurements[measurementKeys[i]] = measurementValues[i];
        }
        
        // Update project impact score
        _updateProjectImpactScore(milestone.projectId, actualValue);
        
        // Distribute milestone funding
        _distributeMilestoneFunding(milestoneId);
        
        emit MilestoneCompleted(milestoneId, actualValue, msg.sender);
    }
    
    /**
     * @dev Issue impact certificate
     */
    function issueImpactCertificate(
        uint256 projectId,
        address recipient,
        string calldata impactType,
        uint256 impactValue,
        string calldata unit,
        uint256 validityPeriod,
        string calldata metadataURI
    ) external onlyVerifiedVerifier projectExists(projectId) returns (uint256) {
        uint256 certificateId = certificateCount++;
        
        certificates[certificateId] = ImpactCertificate({
            certificateId: certificateId,
            projectId: projectId,
            recipient: recipient,
            impactType: impactType,
            impactValue: impactValue,
            unit: unit,
            issuanceDate: block.timestamp,
            validityPeriod: validityPeriod,
            isActive: true,
            metadataURI: metadataURI
        });
        
        emit ImpactCertificateIssued(certificateId, projectId, recipient, impactValue);
        return certificateId;
    }
    
    /**
     * @dev Update biodiversity metrics for project
     */
    function updateBiodiversityMetrics(
        uint256 projectId,
        BiodiversityMetrics calldata metrics
    ) external onlyVerifiedVerifier projectExists(projectId) {
        biodiversityData[projectId] = metrics;
        
        // Update global biodiversity tracking
        globalMetrics["totalSpeciesProtected"] += metrics.speciesCount;
        globalMetrics["totalHabitatProtected"] += metrics.habitatArea;
    }
    
    /**
     * @dev Update community impact metrics
     */
    function updateCommunityImpact(
        uint256 projectId,
        CommunityImpact calldata impact
    ) external onlyVerifiedVerifier projectExists(projectId) {
        communityImpacts[projectId] = impact;
        
        // Update global community impact tracking
        globalMetrics["totalBeneficiaries"] += impact.beneficiaryCount;
        globalMetrics["totalJobsCreated"] += impact.jobsCreated;
        globalMetrics["totalIncomeGenerated"] += impact.incomeGenerated;
    }
    
    /**
     * @dev Submit stakeholder report
     */
    function submitStakeholderReport(
        uint256 projectId,
        string calldata reportType,
        string calldata description,
        string calldata evidenceURI
    ) external projectExists(projectId) returns (uint256) {
        uint256 reportId = projectReports[projectId].length;
        
        projectReports[projectId].push(StakeholderReport({
            reportId: reportId,
            projectId: projectId,
            reporter: msg.sender,
            reportType: reportType,
            description: description,
            evidenceURI: evidenceURI,
            timestamp: block.timestamp,
            isVerified: false,
            verifier: address(0)
        }));
        
        emit StakeholderReportSubmitted(reportId, projectId, msg.sender);
        return reportId;
    }
    
    /**
     * @dev Verify stakeholder report
     */
    function verifyStakeholderReport(
        uint256 projectId,
        uint256 reportId
    ) external onlyVerifiedVerifier projectExists(projectId) {
        require(reportId < projectReports[projectId].length, "EnvironmentalImpact: Report not exists");
        
        StakeholderReport storage report = projectReports[projectId][reportId];
        report.isVerified = true;
        report.verifier = msg.sender;
        
        // Reward reporter for verified report
        _distributeStakeholderReward(report.reporter, 0.01 ether, "Verified report submission");
    }
    
    /**
     * @dev Approve project for funding
     */
    function approveProject(uint256 projectId) external onlyRole(ADMIN_ROLE) projectExists(projectId) {
        projects[projectId].status = ProjectStatus.APPROVED;
    }
    
    /**
     * @dev Certify verifier
     */
    function certifyVerifier(
        address verifier,
        string calldata credentials
    ) external onlyRole(ADMIN_ROLE) {
        verifiedVerifiers[verifier] = true;
        verifierCredentials[verifier] = credentials;
        
        emit VerifierCertified(verifier, credentials);
    }
    
    /**
     * @dev Distribute rewards to stakeholders
     */
    function distributeRewards(
        address[] calldata recipients,
        uint256[] calldata amounts,
        string[] calldata reasons
    ) external onlyRole(ADMIN_ROLE) {
        require(recipients.length == amounts.length && amounts.length == reasons.length, "EnvironmentalImpact: Arrays length mismatch");
        
        uint256 totalAmount = 0;
        for (uint256 i = 0; i < amounts.length; i++) {
            totalAmount += amounts[i];
        }
        
        require(totalAmount <= rewardPool, "EnvironmentalImpact: Insufficient reward pool");
        
        for (uint256 i = 0; i < recipients.length; i++) {
            _distributeStakeholderReward(recipients[i], amounts[i], reasons[i]);
        }
        
        rewardPool -= totalAmount;
    }
    
    /**
     * @dev Get project details
     */
    function getProjectDetails(uint256 projectId) external view projectExists(projectId) returns (
        ProjectType projectType,
        ProjectStatus status,
        address projectOwner,
        string memory name,
        uint256 raisedFunds,
        uint256 targetBudget,
        uint256 totalImpactScore,
        bool isVerified
    ) {
        EnvironmentalProject storage project = projects[projectId];
        
        return (
            project.projectType,
            project.status,
            project.projectOwner,
            project.name,
            project.raisedFunds,
            project.targetBudget,
            project.totalImpactScore,
            project.isVerified
        );
    }
    
    /**
     * @dev Get milestone details
     */
    function getMilestoneDetails(uint256 milestoneId) external view returns (
        uint256 projectId,
        string memory description,
        uint256 targetValue,
        uint256 actualValue,
        MilestoneStatus status,
        address verifier
    ) {
        require(milestoneId < milestoneCount, "EnvironmentalImpact: Milestone not exists");
        ImpactMilestone storage milestone = milestones[milestoneId];
        
        return (
            milestone.projectId,
            milestone.description,
            milestone.targetValue,
            milestone.actualValue,
            milestone.status,
            milestone.verifier
        );
    }
    
    /**
     * @dev Get global impact statistics
     */
    function getGlobalImpactStats() external view returns (
        uint256 totalProjects,
        uint256 completedProjects,
        uint256 totalFunding,
        uint256 totalSpeciesProtected,
        uint256 totalBeneficiaries
    ) {
        return (
            projectCount,
            totalProjectsCompleted,
            totalFundsDeployed,
            globalMetrics["totalSpeciesProtected"],
            globalMetrics["totalBeneficiaries"]
        );
    }
    
    /**
     * @dev Get project reports
     */
    function getProjectReports(uint256 projectId) external view projectExists(projectId) returns (uint256) {
        return projectReports[projectId].length;
    }
    
    // Internal functions
    
    function _updateProjectImpactScore(uint256 projectId, uint256 additionalScore) internal {
        projects[projectId].totalImpactScore += additionalScore;
        typeImpactScores[projects[projectId].projectType] += additionalScore;
    }
    
    function _distributeMilestoneFunding(uint256 milestoneId) internal {
        ImpactMilestone storage milestone = milestones[milestoneId];
        EnvironmentalProject storage project = projects[milestone.projectId];
        
        if (milestone.fundingAmount > 0 && project.raisedFunds >= milestone.fundingAmount) {
            payable(project.projectOwner).transfer(milestone.fundingAmount);
            project.raisedFunds -= milestone.fundingAmount;
        }
    }
    
    function _distributeStakeholderReward(
        address recipient,
        uint256 amount,
        string memory reason
    ) internal {
        stakeholderRewards[recipient] += amount;
        payable(recipient).transfer(amount);
        
        emit RewardDistributed(recipient, amount, reason);
    }
}