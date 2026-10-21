// WRFCoin User Personas and Journey Definitions

export interface UserPersona {
  id: string;
  name: string;
  description: string;
  goals: string[];
  painPoints: string[];
  techExperience: 'beginner' | 'intermediate' | 'advanced' | 'expert';
  cryptoExperience: 'none' | 'basic' | 'experienced' | 'expert';
  devices: string[];
  browsers: string[];
  networkConditions: string[];
}

export interface UserJourneyStep {
  id: string;
  title: string;
  description: string;
  action: string;
  expectedOutcome: string;
  alternativePaths?: string[];
  errorScenarios?: string[];
  performance?: {
    maxLoadTime: number;
    criticalUserJourney: boolean;
  };
}

export interface UserJourney {
  id: string;
  persona: string;
  title: string;
  description: string;
  preconditions: string[];
  steps: UserJourneyStep[];
  postconditions: string[];
  successCriteria: string[];
  tags: string[];
}

// User Personas
export const USER_PERSONAS: UserPersona[] = [
  // Weather Data Provider Personas
  {
    id: 'weather_iot_newcomer',
    name: 'Alex - IoT Device Owner (Newcomer)',
    description:
      'New IoT device owner connecting weather sensors for the first time',
    goals: [
      'Connect weather station to WRFCoin platform',
      'Start earning rewards from weather data',
      'Understand how the platform works',
      'Set up automatic data sharing',
    ],
    painPoints: [
      'Unfamiliar with blockchain terminology',
      'Concerned about device security',
      'Needs clear setup instructions',
      'Worried about token wallet setup',
    ],
    techExperience: 'intermediate',
    cryptoExperience: 'basic',
    devices: ['desktop', 'mobile'],
    browsers: ['chrome', 'safari'],
    networkConditions: ['wifi', '4g'],
  },
  {
    id: 'weather_meteorologist',
    name: 'Dr. Sarah - Professional Meteorologist',
    description:
      'Experienced meteorologist with professional weather equipment',
    goals: [
      'Maximize data quality and rewards',
      'Integrate multiple weather stations',
      'Access advanced analytics features',
      'Contribute to climate research networks',
    ],
    painPoints: [
      'Needs high-precision data validation',
      'Requires API access for bulk operations',
      'Wants detailed performance metrics',
      'Needs enterprise-grade reliability',
    ],
    techExperience: 'expert',
    cryptoExperience: 'experienced',
    devices: ['desktop', 'tablet'],
    browsers: ['chrome', 'firefox', 'edge'],
    networkConditions: ['wifi', 'ethernet'],
  },
  {
    id: 'citizen_scientist',
    name: 'Mike - Citizen Scientist',
    description: 'Environment enthusiast with basic weather station setup',
    goals: [
      'Contribute to environmental monitoring',
      'Earn modest rewards from data sharing',
      'Learn about weather patterns',
      'Connect with environmental community',
    ],
    painPoints: [
      'Limited budget for equipment',
      'Intermittent internet connectivity',
      'Basic understanding of weather metrics',
      'Needs simple, user-friendly interface',
    ],
    techExperience: 'beginner',
    cryptoExperience: 'none',
    devices: ['mobile', 'tablet'],
    browsers: ['safari', 'chrome'],
    networkConditions: ['3g', 'wifi'],
  },
  {
    id: 'rural_data_provider',
    name: 'Emma - Rural Weather Provider',
    description: 'Rural area provider filling data gaps in sparse regions',
    goals: [
      'Provide valuable data from underserved areas',
      'Earn premium rewards for rare location data',
      'Support local agricultural community',
      'Maintain reliable data stream despite challenges',
    ],
    painPoints: [
      'Limited internet bandwidth',
      'Power supply reliability issues',
      'Remote equipment maintenance challenges',
      'Need for offline data buffering',
    ],
    techExperience: 'intermediate',
    cryptoExperience: 'basic',
    devices: ['desktop', 'mobile'],
    browsers: ['chrome', 'edge'],
    networkConditions: ['satellite', '3g', 'wifi'],
  },

  // DeFi Participant Personas
  {
    id: 'crypto_newcomer',
    name: 'Jordan - Crypto Newcomer',
    description: 'First-time cryptocurrency user exploring DeFi yield farming',
    goals: [
      'Learn about DeFi and yield farming',
      'Start with small investments',
      'Understand risks and rewards',
      'Build confidence in crypto ecosystem',
    ],
    painPoints: [
      'Overwhelmed by DeFi complexity',
      'Concerned about security risks',
      'Needs educational resources',
      'Fear of losing money due to mistakes',
    ],
    techExperience: 'intermediate',
    cryptoExperience: 'none',
    devices: ['desktop', 'mobile'],
    browsers: ['chrome', 'safari'],
    networkConditions: ['wifi', '4g'],
  },
  {
    id: 'experienced_trader',
    name: 'Carlos - Experienced DeFi Trader',
    description: 'Seasoned trader using AMM protocols and advanced strategies',
    goals: [
      'Execute complex trading strategies',
      'Optimize yield farming positions',
      'Access advanced DeFi features',
      'Minimize transaction costs',
    ],
    painPoints: [
      'Needs low-latency trade execution',
      'Requires advanced charting tools',
      'Sensitive to gas fees and slippage',
      'Wants detailed analytics and reporting',
    ],
    techExperience: 'expert',
    cryptoExperience: 'expert',
    devices: ['desktop', 'mobile'],
    browsers: ['chrome', 'firefox'],
    networkConditions: ['ethernet', 'wifi'],
  },
  {
    id: 'institutional_investor',
    name: 'Ms. Chen - Institutional Investor',
    description: 'Managing large positions for institutional fund',
    goals: [
      'Execute large-scale DeFi operations',
      'Maintain regulatory compliance',
      'Access institutional-grade security',
      'Generate consistent returns',
    ],
    painPoints: [
      'Requires enterprise security features',
      'Needs detailed compliance reporting',
      'Large transaction size limitations',
      'Regulatory uncertainty concerns',
    ],
    techExperience: 'expert',
    cryptoExperience: 'expert',
    devices: ['desktop', 'tablet'],
    browsers: ['chrome', 'edge'],
    networkConditions: ['ethernet', 'wifi'],
  },
  {
    id: 'carbon_trader',
    name: 'Elena - Carbon Credit Trader',
    description: 'Environmental investor focused on carbon credit trading',
    goals: [
      'Trade verified carbon credits',
      'Support environmental projects',
      'Access global carbon markets',
      'Track environmental impact',
    ],
    painPoints: [
      'Needs verified carbon credit authenticity',
      'Requires detailed project information',
      'Wants transparent pricing mechanisms',
      'Needs ESG reporting capabilities',
    ],
    techExperience: 'advanced',
    cryptoExperience: 'experienced',
    devices: ['desktop', 'tablet'],
    browsers: ['chrome', 'firefox', 'edge'],
    networkConditions: ['wifi', 'ethernet'],
  },

  // Platform Administrator Personas
  {
    id: 'system_administrator',
    name: 'David - System Administrator',
    description: 'Technical administrator monitoring platform health',
    goals: [
      'Monitor system performance and uptime',
      'Manage user accounts and permissions',
      'Handle technical support escalations',
      'Maintain security protocols',
    ],
    painPoints: [
      'Needs comprehensive monitoring tools',
      'Requires quick incident response',
      'Must maintain 24/7 availability',
      'Handles complex technical issues',
    ],
    techExperience: 'expert',
    cryptoExperience: 'advanced',
    devices: ['desktop', 'mobile'],
    browsers: ['chrome', 'firefox'],
    networkConditions: ['ethernet', 'wifi', '4g'],
  },
  {
    id: 'dao_participant',
    name: 'Lisa - DAO Governance Participant',
    description:
      'Active community member participating in governance decisions',
    goals: [
      'Vote on important platform proposals',
      'Propose governance improvements',
      'Engage with community discussions',
      'Shape platform future direction',
    ],
    painPoints: [
      'Needs clear proposal information',
      'Wants transparent voting processes',
      'Requires timely governance notifications',
      'Seeks meaningful community engagement',
    ],
    techExperience: 'advanced',
    cryptoExperience: 'experienced',
    devices: ['desktop', 'mobile', 'tablet'],
    browsers: ['chrome', 'safari', 'firefox'],
    networkConditions: ['wifi', '4g', 'ethernet'],
  },
  {
    id: 'smart_contract_developer',
    name: 'Raj - Smart Contract Developer',
    description: 'Developer deploying and testing smart contracts on platform',
    goals: [
      'Deploy smart contracts efficiently',
      'Test contract interactions',
      'Debug transaction failures',
      'Optimize gas usage',
    ],
    painPoints: [
      'Needs detailed error debugging tools',
      'Requires comprehensive testing environments',
      'Wants gas optimization insights',
      'Needs reliable deployment processes',
    ],
    techExperience: 'expert',
    cryptoExperience: 'expert',
    devices: ['desktop'],
    browsers: ['chrome', 'firefox'],
    networkConditions: ['ethernet', 'wifi'],
  },
  {
    id: 'support_team_member',
    name: 'Maria - Customer Support Specialist',
    description: 'Support team member helping users troubleshoot issues',
    goals: [
      'Quickly diagnose user problems',
      'Provide effective solutions',
      'Access user account information',
      'Escalate technical issues appropriately',
    ],
    painPoints: [
      'Needs comprehensive user dashboards',
      'Requires quick issue resolution tools',
      'Must understand complex technical issues',
      'Handles frustrated users effectively',
    ],
    techExperience: 'advanced',
    cryptoExperience: 'intermediate',
    devices: ['desktop', 'mobile'],
    browsers: ['chrome', 'edge'],
    networkConditions: ['wifi', 'ethernet'],
  },

  // Cross-Chain User Personas
  {
    id: 'multi_chain_investor',
    name: 'Tom - Multi-Chain Investor',
    description: 'Sophisticated investor operating across multiple blockchains',
    goals: [
      'Bridge assets between chains efficiently',
      'Arbitrage opportunities across chains',
      'Manage multi-chain portfolio',
      'Minimize bridge fees and risks',
    ],
    painPoints: [
      'Complex bridge fee calculations',
      'Bridge timing and reliability concerns',
      'Cross-chain wallet management',
      'Security risks in bridge operations',
    ],
    techExperience: 'expert',
    cryptoExperience: 'expert',
    devices: ['desktop', 'mobile'],
    browsers: ['chrome', 'firefox'],
    networkConditions: ['ethernet', 'wifi'],
  },
  {
    id: 'nft_collector',
    name: 'Sophie - NFT Collector',
    description:
      'Digital art and utility NFT collector focused on computational NFTs',
    goals: [
      'Collect unique computational NFTs',
      'Trade NFTs across different platforms',
      'Access exclusive NFT communities',
      'Understand NFT utility and value',
    ],
    painPoints: [
      'NFT authenticity verification',
      'Cross-platform NFT compatibility',
      'High transaction fees for trades',
      'Complex smart contract interactions',
    ],
    techExperience: 'intermediate',
    cryptoExperience: 'experienced',
    devices: ['desktop', 'mobile', 'tablet'],
    browsers: ['chrome', 'safari'],
    networkConditions: ['wifi', '4g'],
  },
  {
    id: 'arbitrage_trader',
    name: 'Marcus - Cross-Chain Arbitrage Trader',
    description:
      'Professional trader exploiting price differences across chains',
    goals: [
      'Identify arbitrage opportunities quickly',
      'Execute trades with minimal latency',
      'Manage risk across multiple chains',
      'Maximize profit from price differences',
    ],
    painPoints: [
      'Requires real-time price feeds',
      'Bridge delays affect profitability',
      'High gas fees reduce margins',
      'Complex multi-step transactions',
    ],
    techExperience: 'expert',
    cryptoExperience: 'expert',
    devices: ['desktop'],
    browsers: ['chrome'],
    networkConditions: ['ethernet'],
  },
  {
    id: 'bridge_developer',
    name: 'Amanda - Bridge Integration Developer',
    description: 'Developer building applications that use cross-chain bridges',
    goals: [
      'Integrate bridge functionality in apps',
      'Test bridge reliability and performance',
      'Handle bridge failure scenarios',
      'Optimize user experience for bridging',
    ],
    painPoints: [
      'Complex bridge API integrations',
      'Bridge failure handling complexity',
      'User experience challenges',
      'Testing across multiple networks',
    ],
    techExperience: 'expert',
    cryptoExperience: 'expert',
    devices: ['desktop'],
    browsers: ['chrome', 'firefox'],
    networkConditions: ['ethernet', 'wifi'],
  },
];
