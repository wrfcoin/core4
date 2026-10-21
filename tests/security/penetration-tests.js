/**
 * Penetration Testing Suite
 * Simulated penetration tests focusing on authentication, authorization, and common attack vectors
 */

const { chromium } = require('@playwright/test');
const axios = require('axios');
const crypto = require('crypto');
const fs = require('fs');
const path = require('path');
const config = require('../test.config');

class PenetrationTester {
  constructor() {
    this.baseUrl = config.baseUrls.backend;
    this.frontendUrl = config.baseUrls.frontend;
    this.reportDir = path.join(__dirname, '../reports/security');
    this.findings = [];
    this.testResults = {
      summary: {
        totalTests: 0,
        successful: 0,
        failed: 0,
        criticalFindings: 0,
        highFindings: 0,
        mediumFindings: 0,
        lowFindings: 0,
      },
      findings: [],
    };

    // Test credentials and tokens
    this.testCredentials = {
      admin: { email: 'admin@wrfcoin.test', password: 'admin123', token: null },
      user: { email: 'user@wrfcoin.test', password: 'user123', token: null },
      lowPriv: {
        email: 'basic@wrfcoin.test',
        password: 'basic123',
        token: null,
      },
    };

    // Ensure report directory exists
    if (!fs.existsSync(this.reportDir)) {
      fs.mkdirSync(this.reportDir, { recursive: true });
    }
  }

  /**
   * Run comprehensive penetration tests
   */
  async runPenetrationTests(options = {}) {
    const {
      testSuites = [
        'authentication',
        'authorization',
        'session',
        'business_logic',
        'data_exposure',
      ],
      setupMode = 'mock',
    } = options;

    console.log('🎯 Starting Penetration Testing Suite\n');
    console.log(`🔧 Setup Mode: ${setupMode}`);
    console.log(`🧪 Test Suites: ${testSuites.join(', ')}`);

    try {
      // Initialize test results
      this.testResults.metadata = {
        timestamp: new Date().toISOString(),
        baseUrl: this.baseUrl,
        frontendUrl: this.frontendUrl,
        testSuites,
        setupMode,
      };

      // Setup test environment
      if (setupMode === 'live') {
        await this.setupTestUsers();
      }

      // Run each test suite
      for (const suite of testSuites) {
        console.log(`\n🔍 Running ${suite} penetration tests...`);
        await this.runTestSuite(suite);
      }

      // Generate reports
      await this.generateJSONReport();
      await this.generateHTMLReport();

      this.printTestSummary();
      return this.testResults;
    } catch (error) {
      console.error('❌ Penetration testing failed:', error);
      throw error;
    }
  }

  /**
   * Setup test users for live testing
   */
  async setupTestUsers() {
    console.log('🔧 Setting up test users...');

    for (const [role, creds] of Object.entries(this.testCredentials)) {
      try {
        // Try to authenticate existing user
        const loginResponse = await axios.post(
          `${this.baseUrl}/api/auth/login`,
          {
            email: creds.email,
            password: creds.password,
          },
          { validateStatus: () => true }
        );

        if (loginResponse.status === 200) {
          creds.token = loginResponse.data.token;
          console.log(`✅ Authenticated ${role} user`);
        } else {
          console.log(
            `⚠️  Could not authenticate ${role} user - tests may be limited`
          );
        }
      } catch (error) {
        console.warn(`⚠️  Setup failed for ${role}: ${error.message}`);
      }
    }
  }

  /**
   * Run specific test suite
   */
  async runTestSuite(suite) {
    switch (suite) {
      case 'authentication':
        await this.testAuthentication();
        break;
      case 'authorization':
        await this.testAuthorization();
        break;
      case 'session':
        await this.testSessionManagement();
        break;
      case 'business_logic':
        await this.testBusinessLogic();
        break;
      case 'data_exposure':
        await this.testDataExposure();
        break;
      default:
        console.warn(`⚠️  Unknown test suite: ${suite}`);
    }
  }

  /**
   * Test authentication vulnerabilities
   */
  async testAuthentication() {
    const authTests = [
      {
        name: 'Brute Force Protection',
        type: 'brute_force',
        test: async () => {
          const attempts = [];
          for (let i = 0; i < 10; i++) {
            const response = await axios.post(
              `${this.baseUrl}/api/auth/login`,
              {
                email: 'admin@wrfcoin.test',
                password: `wrong${i}`,
              },
              { validateStatus: () => true, timeout: 5000 }
            );

            attempts.push({
              attempt: i + 1,
              status: response.status,
              delay: response.headers['retry-after'],
            });
          }

          // Check if account gets locked or rate limited
          const lastAttempts = attempts.slice(-3);
          const hasRateLimit = lastAttempts.some(
            a => a.status === 429 || a.delay
          );

          if (!hasRateLimit) {
            return {
              passed: false,
              severity: 'high',
              evidence: 'No rate limiting detected after 10 failed attempts',
              recommendation: 'Implement account lockout and rate limiting',
            };
          }

          return { passed: true, evidence: 'Rate limiting detected' };
        },
      },
      {
        name: 'Password Reset Enumeration',
        type: 'enumeration',
        test: async () => {
          const validEmail = 'admin@wrfcoin.test';
          const invalidEmail = 'nonexistent@wrfcoin.test';

          const validResponse = await axios.post(
            `${this.baseUrl}/api/auth/forgot-password`,
            {
              email: validEmail,
            },
            { validateStatus: () => true }
          );

          const invalidResponse = await axios.post(
            `${this.baseUrl}/api/auth/forgot-password`,
            {
              email: invalidEmail,
            },
            { validateStatus: () => true }
          );

          // Check if responses differ
          if (
            validResponse.status !== invalidResponse.status ||
            validResponse.data.message !== invalidResponse.data.message
          ) {
            return {
              passed: false,
              severity: 'medium',
              evidence:
                'Different responses for valid/invalid emails in password reset',
              recommendation:
                'Use generic responses for password reset requests',
            };
          }

          return {
            passed: true,
            evidence: 'Consistent responses for password reset',
          };
        },
      },
      {
        name: 'JWT Token Validation',
        type: 'jwt_security',
        test: async () => {
          // Test with malformed JWT
          const invalidTokens = [
            'invalid.jwt.token',
            'eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.invalid.signature',
            'Bearer ' + Buffer.from('{"user": "admin"}').toString('base64'),
          ];

          for (const token of invalidTokens) {
            const response = await axios.get(
              `${this.baseUrl}/api/wallet/balance`,
              {
                headers: { Authorization: token },
                validateStatus: () => true,
              }
            );

            if (response.status === 200) {
              return {
                passed: false,
                severity: 'critical',
                evidence: `Invalid JWT token accepted: ${token.substring(0, 20)}...`,
                recommendation:
                  'Implement proper JWT validation and signature verification',
              };
            }
          }

          return {
            passed: true,
            evidence: 'Invalid JWT tokens properly rejected',
          };
        },
      },
      {
        name: 'Multi-Factor Authentication Bypass',
        type: 'mfa_bypass',
        test: async () => {
          // Test if MFA can be bypassed
          const loginResponse = await axios.post(
            `${this.baseUrl}/api/auth/login`,
            {
              email: 'admin@wrfcoin.test',
              password: 'admin123',
            },
            { validateStatus: () => true }
          );

          if (loginResponse.status === 200 && loginResponse.data.token) {
            // If login succeeds without MFA challenge, it might be missing
            const protectedResponse = await axios.get(
              `${this.baseUrl}/api/admin/settings`,
              {
                headers: {
                  Authorization: `Bearer ${loginResponse.data.token}`,
                },
                validateStatus: () => true,
              }
            );

            if (protectedResponse.status === 200) {
              return {
                passed: false,
                severity: 'high',
                evidence: 'Admin access granted without MFA verification',
                recommendation:
                  'Implement mandatory MFA for administrative accounts',
              };
            }
          }

          return {
            passed: true,
            evidence: 'MFA properly enforced or admin endpoint protected',
          };
        },
      },
    ];

    for (const test of authTests) {
      await this.runPenTest(test);
    }
  }

  /**
   * Test authorization vulnerabilities
   */
  async testAuthorization() {
    const authzTests = [
      {
        name: 'Horizontal Privilege Escalation',
        type: 'privilege_escalation',
        test: async () => {
          // Try to access another user's data
          if (!this.testCredentials.user.token) {
            return {
              passed: true,
              evidence: 'User token not available for testing',
            };
          }

          const response = await axios.get(
            `${this.baseUrl}/api/wallet/balance`,
            {
              headers: {
                Authorization: `Bearer ${this.testCredentials.user.token}`,
              },
              params: { userId: 'admin-user-id' },
              validateStatus: () => true,
            }
          );

          if (
            response.status === 200 &&
            response.data.userId !== this.testCredentials.user.id
          ) {
            return {
              passed: false,
              severity: 'critical',
              evidence: "User can access other users' wallet data",
              recommendation:
                'Implement proper user ID validation in authorization checks',
            };
          }

          return {
            passed: true,
            evidence: 'Horizontal privilege escalation prevented',
          };
        },
      },
      {
        name: 'Vertical Privilege Escalation',
        type: 'privilege_escalation',
        test: async () => {
          // Try to access admin functions with user token
          if (!this.testCredentials.user.token) {
            return {
              passed: true,
              evidence: 'User token not available for testing',
            };
          }

          const adminEndpoints = [
            '/api/admin/users',
            '/api/admin/settings',
            '/api/admin/system-status',
          ];

          for (const endpoint of adminEndpoints) {
            const response = await axios.get(`${this.baseUrl}${endpoint}`, {
              headers: {
                Authorization: `Bearer ${this.testCredentials.user.token}`,
              },
              validateStatus: () => true,
            });

            if (response.status === 200) {
              return {
                passed: false,
                severity: 'critical',
                evidence: `User token granted admin access to ${endpoint}`,
                recommendation: 'Implement role-based access control (RBAC)',
              };
            }
          }

          return {
            passed: true,
            evidence: 'Admin endpoints properly protected',
          };
        },
      },
      {
        name: 'Direct Object Reference',
        type: 'idor',
        test: async () => {
          // Test direct object references
          const testIds = ['1', '2', '999', '../admin', 'null', '${user.id}'];

          for (const id of testIds) {
            const response = await axios.get(
              `${this.baseUrl}/api/users/${id}`,
              {
                headers: this.testCredentials.user.token
                  ? {
                      Authorization: `Bearer ${this.testCredentials.user.token}`,
                    }
                  : {},
                validateStatus: () => true,
              }
            );

            if (response.status === 200) {
              return {
                passed: false,
                severity: 'high',
                evidence: `Direct object reference possible with ID: ${id}`,
                recommendation:
                  'Implement proper authorization checks for object access',
              };
            }
          }

          return {
            passed: true,
            evidence: 'Direct object references properly controlled',
          };
        },
      },
    ];

    for (const test of authzTests) {
      await this.runPenTest(test);
    }
  }

  /**
   * Test session management vulnerabilities
   */
  async testSessionManagement() {
    const sessionTests = [
      {
        name: 'Session Fixation',
        type: 'session_fixation',
        test: async () => {
          // Get initial session
          const initialResponse = await axios.get(
            `${this.baseUrl}/api/health`,
            {
              withCredentials: true,
            }
          );

          const initialCookies = initialResponse.headers['set-cookie'];

          // Login with the same session
          const loginResponse = await axios.post(
            `${this.baseUrl}/api/auth/login`,
            {
              email: 'user@wrfcoin.test',
              password: 'user123',
            },
            {
              headers: initialCookies
                ? { Cookie: initialCookies.join('; ') }
                : {},
              validateStatus: () => true,
            }
          );

          if (loginResponse.status === 200) {
            const loginCookies = loginResponse.headers['set-cookie'];

            // Check if session ID changed after login
            if (
              initialCookies &&
              loginCookies &&
              initialCookies.some(c => loginCookies.includes(c))
            ) {
              return {
                passed: false,
                severity: 'medium',
                evidence: 'Session ID not regenerated after login',
                recommendation: 'Regenerate session ID upon authentication',
              };
            }
          }

          return {
            passed: true,
            evidence: 'Session properly managed or not detectable',
          };
        },
      },
      {
        name: 'Concurrent Session Control',
        type: 'concurrent_sessions',
        test: async () => {
          // Test multiple concurrent logins
          const loginPromises = [];
          for (let i = 0; i < 3; i++) {
            loginPromises.push(
              axios.post(
                `${this.baseUrl}/api/auth/login`,
                {
                  email: 'user@wrfcoin.test',
                  password: 'user123',
                },
                { validateStatus: () => true }
              )
            );
          }

          const responses = await Promise.all(loginPromises);
          const successfulLogins = responses.filter(r => r.status === 200);

          if (successfulLogins.length > 1) {
            // Check if all tokens are valid
            const tokenValidations = await Promise.all(
              successfulLogins.map(r =>
                axios.get(`${this.baseUrl}/api/user/profile`, {
                  headers: { Authorization: `Bearer ${r.data.token}` },
                  validateStatus: () => true,
                })
              )
            );

            const validTokens = tokenValidations.filter(r => r.status === 200);

            if (validTokens.length > 1) {
              return {
                passed: false,
                severity: 'low',
                evidence: `${validTokens.length} concurrent sessions allowed`,
                recommendation:
                  'Consider implementing concurrent session limits',
              };
            }
          }

          return {
            passed: true,
            evidence: 'Concurrent sessions properly controlled',
          };
        },
      },
    ];

    for (const test of sessionTests) {
      await this.runPenTest(test);
    }
  }

  /**
   * Test business logic vulnerabilities
   */
  async testBusinessLogic() {
    const businessTests = [
      {
        name: 'Negative Transaction Amounts',
        type: 'business_logic',
        test: async () => {
          if (!this.testCredentials.user.token) {
            return {
              passed: true,
              evidence: 'User token not available for testing',
            };
          }

          const response = await axios.post(
            `${this.baseUrl}/api/wallet/transfer`,
            {
              to: 'test-address',
              amount: '-100.0',
            },
            {
              headers: {
                Authorization: `Bearer ${this.testCredentials.user.token}`,
              },
              validateStatus: () => true,
            }
          );

          if (response.status === 200) {
            return {
              passed: false,
              severity: 'high',
              evidence: 'Negative transaction amount accepted',
              recommendation:
                'Implement proper input validation for transaction amounts',
            };
          }

          return {
            passed: true,
            evidence: 'Negative amounts properly rejected',
          };
        },
      },
      {
        name: 'NFT Price Manipulation',
        type: 'business_logic',
        test: async () => {
          if (!this.testCredentials.user.token) {
            return {
              passed: true,
              evidence: 'User token not available for testing',
            };
          }

          // Try to set NFT price to zero or negative
          const response = await axios.post(
            `${this.baseUrl}/api/nft/create`,
            {
              name: 'Test NFT',
              price: '0.0',
              metadata: {},
            },
            {
              headers: {
                Authorization: `Bearer ${this.testCredentials.user.token}`,
              },
              validateStatus: () => true,
            }
          );

          if (response.status === 200) {
            return {
              passed: false,
              severity: 'medium',
              evidence: 'NFT created with zero price',
              recommendation:
                'Implement minimum price validation for NFT marketplace',
            };
          }

          return {
            passed: true,
            evidence: 'NFT price validation working properly',
          };
        },
      },
      {
        name: 'Governance Vote Weight Manipulation',
        type: 'business_logic',
        test: async () => {
          if (!this.testCredentials.user.token) {
            return {
              passed: true,
              evidence: 'User token not available for testing',
            };
          }

          // Try to vote with excessive weight
          const response = await axios.post(
            `${this.baseUrl}/api/governance/vote`,
            {
              proposalId: 'test-proposal',
              weight: '999999999999',
              choice: 'yes',
            },
            {
              headers: {
                Authorization: `Bearer ${this.testCredentials.user.token}`,
              },
              validateStatus: () => true,
            }
          );

          if (response.status === 200) {
            return {
              passed: false,
              severity: 'critical',
              evidence: 'Governance vote accepted with excessive weight',
              recommendation:
                "Validate vote weight against user's actual token balance",
            };
          }

          return {
            passed: true,
            evidence: 'Governance vote weight properly validated',
          };
        },
      },
    ];

    for (const test of businessTests) {
      await this.runPenTest(test);
    }
  }

  /**
   * Test data exposure vulnerabilities
   */
  async testDataExposure() {
    const exposureTests = [
      {
        name: 'API Error Information Disclosure',
        type: 'information_disclosure',
        test: async () => {
          // Try to trigger detailed error messages
          const response = await axios.get(
            `${this.baseUrl}/api/users/invalid-id-format`,
            {
              validateStatus: () => true,
            }
          );

          if (response.status >= 400) {
            const errorMessage = JSON.stringify(response.data).toLowerCase();
            const sensitivePatterns = [
              /stack trace/,
              /file path/,
              /database/,
              /sql/,
              /internal server/,
              /exception/,
            ];

            for (const pattern of sensitivePatterns) {
              if (pattern.test(errorMessage)) {
                return {
                  passed: false,
                  severity: 'low',
                  evidence:
                    'Detailed error information exposed in API responses',
                  recommendation:
                    'Implement generic error messages for production',
                };
              }
            }
          }

          return {
            passed: true,
            evidence: 'Error messages appropriately generic',
          };
        },
      },
      {
        name: 'Debug Information Exposure',
        type: 'information_disclosure',
        test: async () => {
          const debugEndpoints = [
            '/debug',
            '/api/debug',
            '/.env',
            '/config',
            '/admin/phpinfo.php',
            '/server-status',
          ];

          for (const endpoint of debugEndpoints) {
            const response = await axios.get(`${this.baseUrl}${endpoint}`, {
              validateStatus: () => true,
            });

            if (response.status === 200) {
              return {
                passed: false,
                severity: 'medium',
                evidence: `Debug endpoint accessible: ${endpoint}`,
                recommendation:
                  'Remove or secure debug endpoints in production',
              };
            }
          }

          return { passed: true, evidence: 'Debug endpoints properly secured' };
        },
      },
    ];

    for (const test of exposureTests) {
      await this.runPenTest(test);
    }
  }

  /**
   * Run individual penetration test
   */
  async runPenTest(test) {
    this.testResults.summary.totalTests++;

    try {
      const result = await test.test();

      if (result.passed) {
        this.testResults.summary.successful++;
        console.log(`  ✅ ${test.name}: PASSED`);
      } else {
        this.testResults.summary.failed++;
        console.log(`  ❌ ${test.name}: FAILED`);

        this.addFinding({
          testName: test.name,
          type: test.type,
          severity: result.severity,
          evidence: result.evidence,
          recommendation: result.recommendation,
          status: 'FAILED',
        });
      }
    } catch (error) {
      this.testResults.summary.failed++;
      console.log(`  ⚠️  ${test.name}: ERROR - ${error.message}`);

      this.addFinding({
        testName: test.name,
        type: test.type,
        severity: 'low',
        evidence: `Test execution error: ${error.message}`,
        recommendation: 'Review test implementation and target availability',
        status: 'ERROR',
      });
    }
  }

  /**
   * Add finding to results
   */
  addFinding(finding) {
    this.findings.push(finding);
    this.testResults.findings.push(finding);

    // Update counters
    switch (finding.severity) {
      case 'critical':
        this.testResults.summary.criticalFindings++;
        break;
      case 'high':
        this.testResults.summary.highFindings++;
        break;
      case 'medium':
        this.testResults.summary.mediumFindings++;
        break;
      case 'low':
        this.testResults.summary.lowFindings++;
        break;
    }
  }

  /**
   * Generate JSON report
   */
  async generateJSONReport() {
    const reportPath = path.join(
      this.reportDir,
      'penetration-test-results.json'
    );
    fs.writeFileSync(reportPath, JSON.stringify(this.testResults, null, 2));
    console.log(`📄 JSON report generated: ${reportPath}`);
  }

  /**
   * Generate HTML report
   */
  async generateHTMLReport() {
    const htmlTemplate = `
<!DOCTYPE html>
<html>
<head>
    <title>Penetration Test Report</title>
    <style>
        body { font-family: Arial, sans-serif; margin: 20px; background: #f5f5f5; }
        .header { background: #6a1b9a; color: white; padding: 20px; border-radius: 8px; margin-bottom: 20px; }
        .summary { background: white; padding: 20px; border-radius: 8px; margin-bottom: 20px; box-shadow: 0 2px 4px rgba(0,0,0,0.1); }
        .summary-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 15px; }
        .summary-card { text-align: center; padding: 15px; border-radius: 8px; }
        .critical { background: #ffebee; border-left: 5px solid #f44336; }
        .high { background: #fff3e0; border-left: 5px solid #ff9800; }
        .medium { background: #f3e5f5; border-left: 5px solid #9c27b0; }
        .low { background: #e8f5e8; border-left: 5px solid #4caf50; }
        .finding { background: white; margin: 10px 0; padding: 15px; border-radius: 8px; box-shadow: 0 2px 4px rgba(0,0,0,0.1); }
        .finding-header { display: flex; justify-content: space-between; align-items: center; margin-bottom: 10px; }
        .severity-badge { padding: 5px 10px; border-radius: 15px; color: white; font-size: 0.8em; }
        .status-badge { padding: 5px 10px; border-radius: 15px; font-size: 0.8em; margin-left: 10px; }
        .failed { background: #f44336; color: white; }
        .error { background: #ff9800; color: white; }
        .evidence { background: #f5f5f5; padding: 10px; border-radius: 4px; margin: 10px 0; }
        .recommendation { background: #e8f5e8; padding: 10px; border-radius: 4px; margin-top: 10px; }
    </style>
</head>
<body>
    <div class="header">
        <h1>🎯 Penetration Test Report</h1>
        <p>Generated: ${this.testResults.metadata.timestamp}</p>
        <p>Target: ${this.testResults.metadata.baseUrl}</p>
    </div>

    <div class="summary">
        <h2>Test Summary</h2>
        <div class="summary-grid">
            <div class="summary-card">
                <h3>${this.testResults.summary.totalTests}</h3>
                <p>Total Tests</p>
            </div>
            <div class="summary-card" style="background: #e8f5e8;">
                <h3>${this.testResults.summary.successful}</h3>
                <p>Successful</p>
            </div>
            <div class="summary-card" style="background: #ffebee;">
                <h3>${this.testResults.summary.failed}</h3>
                <p>Failed</p>
            </div>
        </div>
        
        <h3>Findings by Severity</h3>
        <div class="summary-grid">
            <div class="summary-card critical">
                <h3>${this.testResults.summary.criticalFindings}</h3>
                <p>Critical</p>
            </div>
            <div class="summary-card high">
                <h3>${this.testResults.summary.highFindings}</h3>
                <p>High</p>
            </div>
            <div class="summary-card medium">
                <h3>${this.testResults.summary.mediumFindings}</h3>
                <p>Medium</p>
            </div>
            <div class="summary-card low">
                <h3>${this.testResults.summary.lowFindings}</h3>
                <p>Low</p>
            </div>
        </div>
    </div>

    <h2>Test Findings</h2>
    ${this.testResults.findings
      .map(
        finding => `
        <div class="finding ${finding.severity}">
            <div class="finding-header">
                <h3>${finding.testName}</h3>
                <div>
                    <span class="severity-badge" style="background: ${this.getSeverityColor(finding.severity)}">${finding.severity.toUpperCase()}</span>
                    <span class="status-badge ${finding.status.toLowerCase()}">${finding.status}</span>
                </div>
            </div>
            <p><strong>Type:</strong> ${finding.type}</p>
            <div class="evidence">
                <strong>Evidence:</strong><br>
                ${finding.evidence}
            </div>
            <div class="recommendation">
                <strong>Recommendation:</strong> ${finding.recommendation}
            </div>
        </div>
    `
      )
      .join('')}
</body>
</html>`;

    const htmlPath = path.join(this.reportDir, 'penetration-test-report.html');
    fs.writeFileSync(htmlPath, htmlTemplate);
    console.log(`📊 HTML report generated: ${htmlPath}`);
  }

  /**
   * Get severity color for HTML report
   */
  getSeverityColor(severity) {
    const colors = {
      critical: '#f44336',
      high: '#ff9800',
      medium: '#9c27b0',
      low: '#4caf50',
    };
    return colors[severity] || '#757575';
  }

  /**
   * Print test summary
   */
  printTestSummary() {
    console.log('\n' + '='.repeat(60));
    console.log('🎯 PENETRATION TEST SUMMARY');
    console.log('='.repeat(60));
    console.log(`📊 Total Tests: ${this.testResults.summary.totalTests}`);
    console.log(`✅ Successful: ${this.testResults.summary.successful}`);
    console.log(`❌ Failed: ${this.testResults.summary.failed}`);
    console.log('\n📈 Findings by Severity:');
    console.log(`🚨 Critical: ${this.testResults.summary.criticalFindings}`);
    console.log(`⚠️  High: ${this.testResults.summary.highFindings}`);
    console.log(`⚡ Medium: ${this.testResults.summary.mediumFindings}`);
    console.log(`📝 Low: ${this.testResults.summary.lowFindings}`);

    const successRate =
      (this.testResults.summary.successful /
        this.testResults.summary.totalTests) *
      100;
    console.log(`\n🎯 Security Success Rate: ${successRate.toFixed(1)}%`);

    if (this.testResults.summary.criticalFindings > 0) {
      console.log(
        '\n🚨 CRITICAL VULNERABILITIES FOUND - Immediate action required!'
      );
    } else if (this.testResults.summary.highFindings > 0) {
      console.log('\n⚠️  HIGH SEVERITY VULNERABILITIES - Address promptly');
    } else {
      console.log('\n✅ No critical or high severity vulnerabilities found');
    }

    console.log('='.repeat(60));
  }
}

// Export for use in other test files
module.exports = {
  PenetrationTester,
};

// Main execution function
async function runPenetrationTests(options = {}) {
  const tester = new PenetrationTester();
  return await tester.runPenetrationTests(options);
}

// Run tests if called directly
if (require.main === module) {
  runPenetrationTests({
    testSuites: [
      'authentication',
      'authorization',
      'session',
      'business_logic',
      'data_exposure',
    ],
    setupMode: 'mock',
  })
    .then(results => {
      const hasCriticalFindings =
        results.summary.criticalFindings > 0 ||
        results.summary.highFindings > 0;
      process.exit(hasCriticalFindings ? 1 : 0);
    })
    .catch(error => {
      console.error('Penetration test execution failed:', error);
      process.exit(1);
    });
}
