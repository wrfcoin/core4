// WRFCoin User Journey Definitions
import { UserJourney } from './user-personas';

export const USER_JOURNEYS: UserJourney[] = [
  // Weather Data Provider Journeys
  {
    id: 'iot_newcomer_setup',
    persona: 'weather_iot_newcomer',
    title: 'First-Time IoT Weather Station Setup',
    description:
      'New user connects their weather station and starts earning rewards',
    preconditions: [
      'User has purchased a compatible weather station',
      'User has basic internet connectivity',
      'User has created WRFCoin account',
    ],
    steps: [
      {
        id: 'landing_page_visit',
        title: 'Visit WRFCoin Platform',
        description: 'User visits the platform and sees the main dashboard',
        action: 'Navigate to https://app.wrfcoin.com',
        expectedOutcome:
          'Landing page loads with clear call-to-action for weather providers',
        performance: { maxLoadTime: 3000, criticalUserJourney: true },
      },
      {
        id: 'account_registration',
        title: 'Create User Account',
        description: 'User registers for new account with email verification',
        action: 'Fill registration form and verify email',
        expectedOutcome: 'Account created successfully with welcome email',
        alternativePaths: ['Social login options', 'Web3 wallet connection'],
        errorScenarios: [
          'Email already exists',
          'Invalid email format',
          'Verification email not received',
        ],
      },
      {
        id: 'wallet_setup',
        title: 'Set Up Crypto Wallet',
        description: 'User creates or connects crypto wallet for rewards',
        action: 'Connect MetaMask or create new wallet via platform',
        expectedOutcome: 'Wallet connected with test tokens for gas fees',
        alternativePaths: [
          'Hardware wallet connection',
          'Mobile wallet integration',
        ],
        errorScenarios: [
          'Wallet connection rejected',
          'Network mismatch',
          'Insufficient gas funds',
        ],
      },
      {
        id: 'device_registration',
        title: 'Register Weather Station',
        description: 'User adds their weather station to the platform',
        action: 'Navigate to device management and add new weather station',
        expectedOutcome:
          'Device registered with unique ID and setup instructions',
        alternativePaths: ['Bulk device import', 'API integration setup'],
        errorScenarios: [
          'Device not supported',
          'Invalid device ID',
          'Location verification failed',
        ],
      },
      {
        id: 'location_verification',
        title: 'Verify Device Location',
        description: 'User confirms precise location of weather station',
        action: 'Provide GPS coordinates or select location on map',
        expectedOutcome: 'Location verified and station shows on network map',
        alternativePaths: ['Photo verification', 'GPS auto-detection'],
        errorScenarios: [
          'GPS coordinates invalid',
          'Location already occupied',
          'Verification photo rejected',
        ],
      },
      {
        id: 'data_flow_test',
        title: 'Test Data Transmission',
        description: 'User verifies weather data is being received by platform',
        action: 'Check device dashboard for incoming data',
        expectedOutcome: 'Live weather data visible with quality metrics',
        alternativePaths: ['Manual data entry test', 'Historical data upload'],
        errorScenarios: [
          'No data received',
          'Data quality too low',
          'Connection timeout',
        ],
      },
      {
        id: 'reward_setup',
        title: 'Configure Reward Settings',
        description: 'User sets up automatic reward distribution',
        action: 'Configure reward wallet and payout frequency',
        expectedOutcome: 'Rewards configured with first test payout',
        alternativePaths: ['Manual reward claiming', 'Staking reward tokens'],
        errorScenarios: [
          'Invalid reward wallet',
          'Minimum payout not met',
          'Smart contract interaction failed',
        ],
      },
      {
        id: 'first_reward_claim',
        title: 'Claim First Rewards',
        description:
          'User receives and claims their first weather data rewards',
        action: 'Wait for reward period and claim via dashboard',
        expectedOutcome: 'Rewards successfully transferred to user wallet',
        performance: { maxLoadTime: 5000, criticalUserJourney: true },
      },
    ],
    postconditions: [
      'Weather station actively providing data',
      'User receiving regular rewards',
      'Device visible on network map',
      'Account fully configured',
    ],
    successCriteria: [
      'Data quality score > 75%',
      'Zero missed data transmissions in first 24 hours',
      'First reward payout completed successfully',
      'User satisfaction score > 4/5',
    ],
    tags: ['weather-provider', 'onboarding', 'iot', 'newcomer'],
  },

  {
    id: 'meteorologist_advanced_setup',
    persona: 'weather_meteorologist',
    title: 'Professional Meteorologist Multi-Station Integration',
    description:
      'Expert user integrates multiple professional weather stations with advanced features',
    preconditions: [
      'User has professional weather equipment',
      'User has API development experience',
      'User has established WRFCoin account',
    ],
    steps: [
      {
        id: 'api_access_request',
        title: 'Request Enterprise API Access',
        description:
          'User applies for advanced API features and higher rate limits',
        action: 'Submit enterprise access form with credentials',
        expectedOutcome: 'API access approved with enterprise rate limits',
        performance: { maxLoadTime: 2000, criticalUserJourney: true },
      },
      {
        id: 'bulk_device_import',
        title: 'Import Multiple Weather Stations',
        description: 'User registers multiple professional weather stations',
        action: 'Upload CSV file with device specifications and locations',
        expectedOutcome: 'All devices registered and validated successfully',
        alternativePaths: [
          'API-based bulk import',
          'Manual device-by-device setup',
        ],
        errorScenarios: [
          'CSV format invalid',
          'Device conflicts detected',
          'Location verification failed',
        ],
      },
      {
        id: 'advanced_quality_setup',
        title: 'Configure Quality Assurance Parameters',
        description: 'User sets up advanced data quality monitoring and alerts',
        action: 'Configure QA rules, thresholds, and notification preferences',
        expectedOutcome: 'Quality monitoring active with custom alert system',
        alternativePaths: [
          'Default quality settings',
          'ML-based quality detection',
        ],
        errorScenarios: [
          'Invalid QA parameters',
          'Alert system configuration failed',
        ],
      },
      {
        id: 'api_integration_test',
        title: 'Test API Data Integration',
        description:
          'User verifies custom API integration for bulk data submission',
        action: 'Submit test data batch via API and verify processing',
        expectedOutcome:
          'API integration working with all devices providing data',
        alternativePaths: ['Webhook integration', 'Direct database connection'],
        errorScenarios: [
          'API authentication failed',
          'Data format rejected',
          'Rate limit exceeded',
        ],
      },
      {
        id: 'analytics_dashboard_setup',
        title: 'Configure Advanced Analytics',
        description: 'User sets up comprehensive analytics dashboard',
        action: 'Create custom dashboards with performance metrics',
        expectedOutcome:
          'Analytics dashboard showing all weather stations and performance',
        performance: { maxLoadTime: 4000, criticalUserJourney: false },
      },
    ],
    postconditions: [
      'Multiple weather stations providing high-quality data',
      'API integration fully functional',
      'Advanced analytics dashboard operational',
      'Quality assurance system monitoring all devices',
    ],
    successCriteria: [
      'All devices maintaining >95% data quality',
      'API uptime >99.9%',
      'Zero data loss incidents',
      'Advanced features utilized effectively',
    ],
    tags: ['weather-provider', 'professional', 'api', 'enterprise'],
  },

  // DeFi Participant Journeys
  {
    id: 'crypto_newcomer_defi_exploration',
    persona: 'crypto_newcomer',
    title: 'First-Time DeFi Yield Farming Experience',
    description:
      'Crypto newcomer explores DeFi features and starts yield farming',
    preconditions: [
      'User has basic understanding of cryptocurrency',
      'User has connected wallet with some tokens',
      'User has completed platform onboarding',
    ],
    steps: [
      {
        id: 'defi_education_hub',
        title: 'Access DeFi Education Resources',
        description:
          'User learns about DeFi concepts through platform education hub',
        action: 'Navigate to education section and complete DeFi basics course',
        expectedOutcome:
          'User understands yield farming, liquidity pools, and risks',
        performance: { maxLoadTime: 3000, criticalUserJourney: true },
      },
      {
        id: 'practice_mode_enable',
        title: 'Enable Practice Mode',
        description:
          'User activates safe practice environment with test tokens',
        action: 'Switch to practice mode for risk-free learning',
        expectedOutcome: 'Practice mode active with test tokens available',
        alternativePaths: ['Skip to live trading', 'Guided tutorial mode'],
        errorScenarios: ['Practice mode unavailable', 'Test tokens not issued'],
      },
      {
        id: 'liquidity_pool_exploration',
        title: 'Explore Liquidity Pools',
        description:
          'User browses available liquidity pools and their APY rates',
        action: 'View liquidity pools page and analyze options',
        expectedOutcome:
          'User identifies suitable low-risk pool for first investment',
        performance: { maxLoadTime: 2500, criticalUserJourney: false },
      },
      {
        id: 'small_liquidity_provision',
        title: 'Provide Liquidity (Small Amount)',
        description: 'User makes first liquidity provision with small amount',
        action: 'Add liquidity to conservative pool with small token amount',
        expectedOutcome:
          'Liquidity provided successfully with LP tokens received',
        alternativePaths: [
          'Single-sided liquidity provision',
          'Stablecoin pairs only',
        ],
        errorScenarios: [
          'Insufficient token balance',
          'High slippage warning',
          'Transaction failed',
        ],
      },
      {
        id: 'yield_monitoring_setup',
        title: 'Set Up Yield Monitoring',
        description:
          'User configures alerts and monitoring for yield performance',
        action:
          'Configure notifications for yield changes and performance alerts',
        expectedOutcome:
          'Monitoring dashboard active with alert preferences set',
        performance: { maxLoadTime: 2000, criticalUserJourney: false },
      },
      {
        id: 'first_yield_harvest',
        title: 'Harvest First Yield',
        description: 'User claims their first yield farming rewards',
        action: 'Claim accumulated rewards from liquidity provision',
        expectedOutcome: 'Yield successfully claimed and transferred to wallet',
        performance: { maxLoadTime: 5000, criticalUserJourney: true },
      },
    ],
    postconditions: [
      'User actively participating in yield farming',
      'User confident with basic DeFi operations',
      'Monitoring system tracking performance',
      'Educational progress tracked and rewarded',
    ],
    successCriteria: [
      'First liquidity provision completed without errors',
      'User demonstrates understanding of risks',
      'Yield harvesting successful',
      'User satisfaction with education resources > 4/5',
    ],
    tags: ['defi', 'newcomer', 'education', 'yield-farming'],
  },

  {
    id: 'experienced_trader_advanced_strategies',
    persona: 'experienced_trader',
    title: 'Advanced DeFi Trading and Optimization',
    description:
      'Experienced trader uses advanced features for complex strategies',
    preconditions: [
      'User has significant DeFi experience',
      'User has substantial token holdings',
      'User familiar with advanced trading concepts',
    ],
    steps: [
      {
        id: 'advanced_dashboard_access',
        title: 'Access Advanced Trading Dashboard',
        description: 'User opens professional trading interface',
        action: 'Switch to advanced mode and access professional dashboard',
        expectedOutcome:
          'Advanced dashboard loaded with all trading tools available',
        performance: { maxLoadTime: 2000, criticalUserJourney: true },
      },
      {
        id: 'arbitrage_opportunity_scan',
        title: 'Scan for Arbitrage Opportunities',
        description: 'User identifies price differences across different pools',
        action: 'Use arbitrage scanner to find profitable opportunities',
        expectedOutcome:
          'Profitable arbitrage opportunities identified and ranked',
        performance: { maxLoadTime: 1500, criticalUserJourney: true },
      },
      {
        id: 'flash_loan_arbitrage',
        title: 'Execute Flash Loan Arbitrage',
        description: 'User executes complex arbitrage using flash loans',
        action: 'Configure and execute flash loan arbitrage transaction',
        expectedOutcome: 'Arbitrage executed successfully with profit realized',
        alternativePaths: [
          'Multi-step manual arbitrage',
          'Automated arbitrage bot',
        ],
        errorScenarios: [
          'Flash loan failed',
          'Price slippage too high',
          'MEV attack detected',
        ],
      },
      {
        id: 'yield_strategy_optimization',
        title: 'Optimize Yield Farming Strategy',
        description: 'User rebalances portfolio for maximum yield efficiency',
        action: 'Analyze and rebalance positions across multiple protocols',
        expectedOutcome:
          'Portfolio optimized with improved risk-adjusted returns',
        performance: { maxLoadTime: 3000, criticalUserJourney: false },
      },
      {
        id: 'risk_management_setup',
        title: 'Configure Advanced Risk Management',
        description: 'User sets up automated risk controls and stop losses',
        action: 'Configure automated rebalancing and risk limits',
        expectedOutcome:
          'Risk management system active with automated controls',
        alternativePaths: [
          'Manual risk monitoring',
          'Third-party risk tools integration',
        ],
      },
    ],
    postconditions: [
      'Advanced trading strategies actively running',
      'Risk management system protecting positions',
      'Portfolio optimized for current market conditions',
      'Automated monitoring and alerts configured',
    ],
    successCriteria: [
      'Arbitrage opportunities successfully executed',
      'Risk-adjusted returns improved by >10%',
      'No significant losses due to risk events',
      'Advanced features utilized effectively',
    ],
    tags: ['defi', 'advanced', 'arbitrage', 'optimization'],
  },

  // Platform Administrator Journeys
  {
    id: 'system_admin_monitoring_dashboard',
    persona: 'system_administrator',
    title: 'System Health Monitoring and Incident Response',
    description:
      'System administrator monitors platform health and responds to incidents',
    preconditions: [
      'Admin has full system access credentials',
      'Monitoring infrastructure is operational',
      'Admin is familiar with platform architecture',
    ],
    steps: [
      {
        id: 'admin_dashboard_login',
        title: 'Access Admin Dashboard',
        description: 'Admin logs into comprehensive monitoring dashboard',
        action: 'Login with admin credentials and access system dashboard',
        expectedOutcome: 'Admin dashboard loaded with real-time system metrics',
        performance: { maxLoadTime: 2000, criticalUserJourney: true },
      },
      {
        id: 'system_health_overview',
        title: 'Review System Health Overview',
        description: 'Admin checks all critical system components',
        action: 'Review service status, performance metrics, and alert summary',
        expectedOutcome: 'Complete system health picture available at a glance',
        performance: { maxLoadTime: 1500, criticalUserJourney: true },
      },
      {
        id: 'performance_analysis',
        title: 'Analyze Performance Metrics',
        description: 'Admin investigates performance trends and bottlenecks',
        action: 'Drill down into specific service metrics and performance data',
        expectedOutcome:
          'Performance issues identified and prioritized for action',
        performance: { maxLoadTime: 3000, criticalUserJourney: false },
      },
      {
        id: 'incident_response_simulation',
        title: 'Handle Simulated Service Incident',
        description: 'Admin responds to simulated service degradation',
        action: 'Follow incident response procedures for service outage',
        expectedOutcome: 'Incident resolved quickly with minimal user impact',
        alternativePaths: [
          'Escalate to development team',
          'Activate backup systems',
        ],
        errorScenarios: [
          'Communication failure',
          'Backup systems unavailable',
          'Data loss detected',
        ],
      },
      {
        id: 'user_account_management',
        title: 'Manage User Accounts and Permissions',
        description: 'Admin handles user account issues and permission updates',
        action:
          'Process account verification, permission changes, and security issues',
        expectedOutcome:
          'User accounts properly managed with security maintained',
        performance: { maxLoadTime: 2000, criticalUserJourney: false },
      },
    ],
    postconditions: [
      'System monitoring dashboard configured and operational',
      'Incident response procedures tested and validated',
      'User account management processes streamlined',
      'Performance optimization recommendations documented',
    ],
    successCriteria: [
      'Mean time to detection (MTTD) < 5 minutes',
      'Mean time to resolution (MTTR) < 30 minutes',
      'Zero critical incidents escalated inappropriately',
      'User satisfaction with admin support > 4.5/5',
    ],
    tags: ['admin', 'monitoring', 'incident-response', 'system-health'],
  },

  // Cross-Chain User Journeys
  {
    id: 'multi_chain_asset_bridge',
    persona: 'multi_chain_investor',
    title: 'Cross-Chain Asset Bridging and Portfolio Management',
    description:
      'Multi-chain investor bridges assets and manages cross-chain portfolio',
    preconditions: [
      'User has assets on multiple blockchain networks',
      'User has wallets configured for target chains',
      'User understands cross-chain bridge risks',
    ],
    steps: [
      {
        id: 'bridge_dashboard_access',
        title: 'Access Cross-Chain Bridge Dashboard',
        description:
          'User opens bridge interface with multi-chain portfolio view',
        action: 'Navigate to bridge dashboard and connect multi-chain wallets',
        expectedOutcome:
          'Bridge dashboard showing assets across all connected chains',
        performance: { maxLoadTime: 2500, criticalUserJourney: true },
      },
      {
        id: 'bridge_route_analysis',
        title: 'Analyze Bridge Routes and Fees',
        description:
          'User compares different bridge options for optimal routing',
        action:
          'Compare bridge providers, fees, and estimated completion times',
        expectedOutcome:
          'Optimal bridge route selected based on cost and speed preferences',
        performance: { maxLoadTime: 3000, criticalUserJourney: true },
      },
      {
        id: 'bridge_transaction_execution',
        title: 'Execute Cross-Chain Bridge Transaction',
        description:
          'User initiates asset transfer between blockchain networks',
        action: 'Execute bridge transaction with selected route and parameters',
        expectedOutcome:
          'Bridge transaction initiated and tracking information provided',
        alternativePaths: [
          'Direct bridge protocol interaction',
          'Aggregated bridge routing',
        ],
        errorScenarios: [
          'Bridge capacity exceeded',
          'Source chain congestion',
          'Destination chain issues',
        ],
      },
      {
        id: 'bridge_monitoring',
        title: 'Monitor Bridge Transaction Progress',
        description: 'User tracks bridge transaction through completion',
        action:
          'Monitor transaction status and receive completion notifications',
        expectedOutcome:
          'Transaction completes successfully with assets received on target chain',
        performance: { maxLoadTime: 30000, criticalUserJourney: true },
        errorScenarios: [
          'Bridge transaction stuck',
          'Validator delays',
          'Network finality issues',
        ],
      },
      {
        id: 'portfolio_rebalancing',
        title: 'Rebalance Cross-Chain Portfolio',
        description: 'User optimizes asset allocation across different chains',
        action: 'Analyze portfolio and execute rebalancing transactions',
        expectedOutcome:
          'Portfolio optimally distributed across chains for yield and risk',
        performance: { maxLoadTime: 5000, criticalUserJourney: false },
      },
    ],
    postconditions: [
      'Assets successfully bridged to target chain',
      'Portfolio optimally balanced across chains',
      'Bridge transaction history tracked and recorded',
      'User confident with cross-chain operations',
    ],
    successCriteria: [
      'Bridge transactions complete within estimated timeframes',
      'Zero asset loss during bridging operations',
      'Portfolio optimization improves risk-adjusted returns',
      'User satisfaction with bridge experience > 4/5',
    ],
    tags: ['cross-chain', 'bridge', 'portfolio-management', 'multi-chain'],
  },
];

export default USER_JOURNEYS;
