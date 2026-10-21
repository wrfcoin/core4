/**
 * Security Testing Tools Configuration for Wrfcoin Platform
 */

module.exports = {
  // Target API Servers for Security Scanning
  targets: {
    main: {
      baseUrl: 'http://localhost:4001',
      name: 'Main API Server',
      endpoints: [
        '/health',
        '/api/transactions/create',
        '/api/blockchain/stats',
        '/api/governance/proposal',
        '/api/nft/create',
      ],
    },
    wallet: {
      baseUrl: 'http://localhost:4000',
      name: 'Wallet Server',
      endpoints: [
        '/api/wallet/generate-mnemonic',
        '/api/wallet/validate-mnemonic',
        '/api/wallet/create',
        '/api/wallet/transfer',
      ],
    },
    crossChain: {
      baseUrl: 'http://localhost:3000',
      name: 'Cross-Chain Protocol Server',
      endpoints: [
        '/api/bridge/transfer',
        '/api/bridge/status',
        '/api/bridge/claim',
      ],
    },
    compute: {
      baseUrl: 'http://localhost:6000',
      name: 'Compute Networks Server',
      endpoints: ['/api/compute/submit-job', '/api/compute/status'],
    },
    storage: {
      baseUrl: 'http://localhost:7000',
      name: 'Storage Networks Server',
      endpoints: ['/api/storage/upload', '/api/storage/retrieve'],
    },
  },

  // Static Application Security Testing (SAST)
  sast: {
    semgrep: {
      configPath: './security-tools/sast/semgrep-rules.yml',
      excludePaths: [
        'node_modules/',
        'coverage/',
        'dist/',
        '*.test.js',
        '*.spec.js',
        'fuzz-testing/reports/',
      ],
      ruleSets: [
        'p/typescript',
        'p/nodejs',
        'p/security-audit',
        'p/owasp-top-ten',
        'p/jwt',
      ],
      severityThreshold: 'WARNING',
      outputFormat: 'json',
    },
    sonarqube: {
      enabled: false, // Enable when SonarQube server is available
      serverUrl: 'http://localhost:9000',
      projectKey: 'wrfcoin-blockchain',
    },
  },

  // Dynamic Application Security Testing (DAST)
  dast: {
    zap: {
      dockerImage: 'owasp/zap2docker-stable',
      baselineTimeout: 300, // 5 minutes
      fullScanTimeout: 3600, // 1 hour
      reportFormats: ['HTML', 'XML', 'JSON'],
      alertSeverityThreshold: 'Medium',
      spiderMaxDepth: 5,
      activeScanIntensity: 'LOW', // LOW, MEDIUM, HIGH
      excludeUrls: ['.*/logout.*', '.*/admin.*'],
    },
    nuclei: {
      templatesPath: './security-tools/penetration/nuclei-templates',
      severity: ['critical', 'high', 'medium'],
      tags: ['cve', 'xss', 'sqli', 'rce', 'lfi', 'ssrf'],
      rateLimit: 150, // requests per second
      timeout: 10, // seconds
      retries: 2,
    },
  },

  // Software Composition Analysis (SCA)
  sca: {
    npm: {
      auditLevel: 'moderate',
      productionOnly: false,
      skipDevDependencies: false,
    },
    snyk: {
      enabled: true,
      severityThreshold: 'high',
      monitor: true,
      testDevDependencies: true,
      dockerScan: true,
    },
    dependencyCheck: {
      enabled: true,
      suppressionFile: './security-tools/sca/suppression.xml',
      cveUrlModified:
        'https://nvd.nist.gov/feeds/json/cve/1.1/nvdcve-1.1-modified.json.gz',
      cveUrlBase:
        'https://nvd.nist.gov/feeds/json/cve/1.1/nvdcve-1.1-%d.json.gz',
    },
  },

  // Container Security Scanning
  container: {
    trivy: {
      format: 'json',
      severity: ['CRITICAL', 'HIGH', 'MEDIUM'],
      vulnType: ['os', 'library'],
      ignoreUnfixed: false,
      timeout: '10m',
      cacheTTL: '24h',
    },
    dockerBench: {
      enabled: true,
      excludeChecks: [], // CIS Docker Benchmark checks to exclude
      outputFormat: 'json',
    },
  },

  // Infrastructure Security
  infrastructure: {
    checkov: {
      framework: ['terraform', 'kubernetes', 'dockerfile'],
      checkTypes: ['CKV_*'],
      skipCheck: [],
      compactMode: false,
    },
    prowler: {
      enabled: false, // Enable for AWS environments
      checks: ['cis_level1', 'cis_level2'],
      regions: ['us-east-1', 'us-west-2'],
      outputFormats: ['json', 'html'],
    },
  },

  // Compliance and Reporting
  compliance: {
    frameworks: ['OWASP-ASVS', 'PCI-DSS', 'ISO-27001', 'SOX', 'NIST-CSF'],
    reportingFrequency: 'daily',
    alertChannels: ['email', 'slack'],
    riskScoring: {
      critical: 10,
      high: 7,
      medium: 4,
      low: 1,
    },
  },

  // DefectDojo Integration
  defectDojo: {
    enabled: false, // Enable when DefectDojo is deployed
    apiUrl: 'http://localhost:8080/api/v2',
    productId: 1,
    engagementName: 'Wrfcoin Security Testing',
    testTypes: {
      semgrep: 'Semgrep Scan',
      zap: 'ZAP Scan',
      snyk: 'Dependency Check',
      trivy: 'Container Scan',
      nuclei: 'Nuclei Scan',
    },
  },

  // Security Headers Testing
  securityHeaders: {
    requiredHeaders: [
      'X-Content-Type-Options',
      'X-Frame-Options',
      'X-XSS-Protection',
      'Strict-Transport-Security',
      'Content-Security-Policy',
      'Referrer-Policy',
    ],
    cspValidation: true,
    hstsValidation: true,
  },

  // API Security Testing
  apiSecurity: {
    authenticationTests: true,
    authorizationTests: true,
    inputValidationTests: true,
    rateLimitingTests: true,
    cors: {
      validateOrigins: true,
      allowedMethods: ['GET', 'POST', 'PUT', 'DELETE'],
      allowedHeaders: ['Content-Type', 'Authorization'],
    },
  },

  // Blockchain-Specific Security Tests
  blockchain: {
    consensusTests: true,
    smartContractTests: true,
    walletSecurityTests: true,
    crossChainBridgeTests: true,
    miningSecurityTests: true,
    governanceSecurityTests: true,
  },

  // Reporting Configuration
  reporting: {
    outputDir: './reports/security',
    formats: ['html', 'json', 'csv', 'pdf'],
    includeExecutiveSummary: true,
    includeRemediationGuidance: true,
    includeComplianceMapping: true,
    retentionDays: 90,
  },

  // CI/CD Integration
  cicd: {
    failOnCritical: true,
    failOnHigh: false,
    maxAllowedHigh: 5,
    maxAllowedMedium: 20,
    generateGitHubIssues: true,
    slackNotifications: true,
  },
};
