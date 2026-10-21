/**
 * Dynamic Application Security Testing (DAST) Scanner
 * Runtime security testing for running applications
 */

const { chromium } = require('@playwright/test');
const axios = require('axios');
const fs = require('fs');
const path = require('path');
const config = require('../test.config');

class DASTScanner {
  constructor() {
    this.baseUrl = config.baseUrls.backend;
    this.frontendUrl = config.baseUrls.frontend;
    this.reportDir = path.join(__dirname, '../reports/security');
    this.findings = [];
    this.scanResults = {
      summary: {
        totalTests: 0,
        vulnerableEndpoints: 0,
        totalFindings: 0,
        criticalCount: 0,
        highCount: 0,
        mediumCount: 0,
        lowCount: 0,
      },
      findings: [],
    };

    // Ensure report directory exists
    if (!fs.existsSync(this.reportDir)) {
      fs.mkdirSync(this.reportDir, { recursive: true });
    }
  }

  /**
   * Run comprehensive DAST scan
   */
  async runComprehensiveScan(options = {}) {
    const {
      scanTypes = [
        'headers',
        'authentication',
        'authorization',
        'injection',
        'xss',
        'csrf',
      ],
      targets = ['api', 'frontend'],
      timeout = 30000,
    } = options;

    console.log('🔍 Starting DAST Security Scan\n');
    console.log(`🎯 API URL: ${this.baseUrl}`);
    console.log(`🎯 Frontend URL: ${this.frontendUrl}`);
    console.log(`🔬 Scan Types: ${scanTypes.join(', ')}`);

    try {
      // Initialize scan results
      this.scanResults.metadata = {
        timestamp: new Date().toISOString(),
        baseUrl: this.baseUrl,
        frontendUrl: this.frontendUrl,
        scanTypes,
        targets,
      };

      // Run each scan type
      for (const scanType of scanTypes) {
        console.log(`\n🔍 Running ${scanType} scan...`);
        await this.runScanType(scanType, targets);
      }

      // Generate reports
      await this.generateJSONReport();
      await this.generateHTMLReport();

      this.printScanSummary();
      return this.scanResults;
    } catch (error) {
      console.error('❌ DAST scan failed:', error);
      throw error;
    }
  }

  /**
   * Run specific scan type
   */
  async runScanType(scanType, targets) {
    switch (scanType) {
      case 'headers':
        await this.scanSecurityHeaders(targets);
        break;
      case 'authentication':
        await this.scanAuthentication();
        break;
      case 'authorization':
        await this.scanAuthorization();
        break;
      case 'injection':
        await this.scanInjectionVulnerabilities();
        break;
      case 'xss':
        await this.scanXSSVulnerabilities();
        break;
      case 'csrf':
        await this.scanCSRFProtection();
        break;
      default:
        console.warn(`⚠️  Unknown scan type: ${scanType}`);
    }
  }

  /**
   * Scan for security headers
   */
  async scanSecurityHeaders(targets) {
    const securityHeaders = [
      'Content-Security-Policy',
      'X-Content-Type-Options',
      'X-Frame-Options',
      'X-XSS-Protection',
      'Strict-Transport-Security',
      'Referrer-Policy',
      'Permissions-Policy',
    ];

    const testUrls = [];
    if (targets.includes('api')) {
      testUrls.push(`${this.baseUrl}/api/health`);
    }
    if (targets.includes('frontend')) {
      testUrls.push(this.frontendUrl);
    }

    for (const url of testUrls) {
      try {
        const response = await axios.get(url, { timeout: 10000 });
        const headers = response.headers;

        for (const header of securityHeaders) {
          const headerKey = header.toLowerCase();
          if (!headers[headerKey]) {
            this.addFinding({
              type: 'security_headers',
              category: 'missing_header',
              severity: this.getHeaderSeverity(header),
              url,
              description: `Missing security header: ${header}`,
              evidence: `Header '${header}' not present in response`,
              recommendation: this.getHeaderRecommendation(header),
            });
          } else {
            // Check header values for weaknesses
            const headerValue = headers[headerKey];
            this.validateHeaderValue(header, headerValue, url);
          }
        }

        // Check for information disclosure headers
        const sensitiveHeaders = ['server', 'x-powered-by', 'x-aspnet-version'];
        for (const header of sensitiveHeaders) {
          if (headers[header]) {
            this.addFinding({
              type: 'security_headers',
              category: 'information_disclosure',
              severity: 'low',
              url,
              description: `Information disclosure via ${header} header`,
              evidence: `${header}: ${headers[header]}`,
              recommendation:
                'Remove or obfuscate server identification headers',
            });
          }
        }
      } catch (error) {
        console.warn(`⚠️  Could not scan headers for ${url}: ${error.message}`);
      }
    }
  }

  /**
   * Validate security header values
   */
  validateHeaderValue(header, value, url) {
    switch (header.toLowerCase()) {
      case 'content-security-policy':
        if (
          value.includes("'unsafe-inline'") ||
          value.includes("'unsafe-eval'")
        ) {
          this.addFinding({
            type: 'security_headers',
            category: 'weak_csp',
            severity: 'medium',
            url,
            description: 'Weak Content Security Policy',
            evidence: `CSP: ${value}`,
            recommendation: 'Remove unsafe-inline and unsafe-eval directives',
          });
        }
        break;
      case 'x-frame-options':
        if (value.toLowerCase() === 'allowall') {
          this.addFinding({
            type: 'security_headers',
            category: 'weak_frame_options',
            severity: 'medium',
            url,
            description: 'Permissive X-Frame-Options',
            evidence: `X-Frame-Options: ${value}`,
            recommendation: 'Use DENY or SAMEORIGIN instead of ALLOWALL',
          });
        }
        break;
    }
  }

  /**
   * Scan authentication mechanisms
   */
  async scanAuthentication() {
    const authTests = [
      {
        name: 'Weak Password Policy',
        endpoint: '/api/auth/register',
        payload: { email: 'test@test.com', password: '123' },
        expectStatus: 400,
      },
      {
        name: 'SQL Injection in Login',
        endpoint: '/api/auth/login',
        payload: { email: "admin'--", password: 'any' },
        expectStatus: [400, 401],
      },
      {
        name: 'Account Enumeration',
        endpoint: '/api/auth/login',
        payload: { email: 'nonexistent@test.com', password: 'wrong' },
        checkResponse: true,
      },
    ];

    for (const test of authTests) {
      try {
        const response = await axios.post(
          `${this.baseUrl}${test.endpoint}`,
          test.payload,
          {
            timeout: 10000,
            validateStatus: () => true, // Accept all status codes
          }
        );

        if (test.expectStatus && !Array.isArray(test.expectStatus)) {
          test.expectStatus = [test.expectStatus];
        }

        if (test.expectStatus && !test.expectStatus.includes(response.status)) {
          this.addFinding({
            type: 'authentication',
            category: 'auth_bypass',
            severity: 'high',
            url: `${this.baseUrl}${test.endpoint}`,
            description: `${test.name} - Unexpected response`,
            evidence: `Expected status ${test.expectStatus}, got ${response.status}`,
            recommendation:
              'Implement proper input validation and error handling',
          });
        }

        if (test.checkResponse && test.name === 'Account Enumeration') {
          // Check if error messages differ for existing vs non-existing accounts
          const existingAccountResponse = await axios.post(
            `${this.baseUrl}${test.endpoint}`,
            { email: 'admin@wrfcoin.com', password: 'wrong' },
            { timeout: 10000, validateStatus: () => true }
          );

          if (response.data.message !== existingAccountResponse.data.message) {
            this.addFinding({
              type: 'authentication',
              category: 'account_enumeration',
              severity: 'medium',
              url: `${this.baseUrl}${test.endpoint}`,
              description: 'Account enumeration via different error messages',
              evidence:
                'Different error messages for existing/non-existing accounts',
              recommendation:
                'Use generic error messages for authentication failures',
            });
          }
        }
      } catch (error) {
        console.warn(`⚠️  Auth test failed for ${test.name}: ${error.message}`);
      }
    }
  }

  /**
   * Scan authorization mechanisms
   */
  async scanAuthorization() {
    const authzTests = [
      {
        name: 'Access without token',
        endpoint: '/api/wallet/balance',
        headers: {},
        expectStatus: 401,
      },
      {
        name: 'Access with invalid token',
        endpoint: '/api/wallet/balance',
        headers: { Authorization: 'Bearer invalid-token' },
        expectStatus: 401,
      },
      {
        name: 'Privilege escalation test',
        endpoint: '/api/admin/users',
        headers: { Authorization: 'Bearer user-token' },
        expectStatus: 403,
      },
    ];

    for (const test of authzTests) {
      try {
        const response = await axios.get(`${this.baseUrl}${test.endpoint}`, {
          headers: test.headers,
          timeout: 10000,
          validateStatus: () => true,
        });

        if (response.status !== test.expectStatus) {
          this.addFinding({
            type: 'authorization',
            category: 'authz_bypass',
            severity: response.status === 200 ? 'critical' : 'medium',
            url: `${this.baseUrl}${test.endpoint}`,
            description: `${test.name} - Authorization bypass`,
            evidence: `Expected status ${test.expectStatus}, got ${response.status}`,
            recommendation: 'Implement proper authorization checks',
          });
        }
      } catch (error) {
        console.warn(
          `⚠️  Authorization test failed for ${test.name}: ${error.message}`
        );
      }
    }
  }

  /**
   * Scan for injection vulnerabilities
   */
  async scanInjectionVulnerabilities() {
    const injectionPayloads = [
      { type: 'SQL', payload: "'; DROP TABLE users; --" },
      { type: 'NoSQL', payload: '{"$ne": null}' },
      { type: 'Command', payload: '; cat /etc/passwd' },
      { type: 'LDAP', payload: '*)(&' },
    ];

    const testEndpoints = [
      '/api/users/search',
      '/api/weather/station',
      '/api/nft/search',
    ];

    for (const endpoint of testEndpoints) {
      for (const { type, payload } of injectionPayloads) {
        try {
          const response = await axios.get(`${this.baseUrl}${endpoint}`, {
            params: { q: payload },
            timeout: 10000,
            validateStatus: () => true,
          });

          // Check for error messages that might indicate injection
          const errorPatterns = [
            /sql/i,
            /syntax error/i,
            /mysql/i,
            /postgresql/i,
            /sqlite/i,
            /mongodb/i,
            /command not found/i,
          ];

          const responseText = JSON.stringify(response.data).toLowerCase();
          for (const pattern of errorPatterns) {
            if (pattern.test(responseText)) {
              this.addFinding({
                type: 'injection',
                category: `${type.toLowerCase()}_injection`,
                severity: 'high',
                url: `${this.baseUrl}${endpoint}`,
                description: `Potential ${type} injection vulnerability`,
                evidence: `Error response indicates ${type} injection: ${payload}`,
                recommendation:
                  'Use parameterized queries and input validation',
              });
              break;
            }
          }
        } catch (error) {
          // Timeout or connection errors might indicate successful injection
          if (error.code === 'ECONNABORTED') {
            this.addFinding({
              type: 'injection',
              category: 'potential_injection',
              severity: 'medium',
              url: `${this.baseUrl}${endpoint}`,
              description: `Potential injection causing timeout`,
              evidence: `Request timeout with payload: ${payload}`,
              recommendation:
                'Investigate server-side processing and implement input validation',
            });
          }
        }
      }
    }
  }

  /**
   * Scan for XSS vulnerabilities using Playwright
   */
  async scanXSSVulnerabilities() {
    const browser = await chromium.launch({ headless: true });
    const context = await browser.newContext();
    const page = await context.newPage();

    const xssPayloads = [
      '<script>alert("XSS")</script>',
      '"><script>alert("XSS")</script>',
      "';alert('XSS');//",
      'javascript:alert("XSS")',
      '<img src=x onerror=alert("XSS")>',
    ];

    const testPages = [
      { url: '/dashboard', inputSelector: '[data-testid="search-input"]' },
      { url: '/nft-marketplace', inputSelector: '[data-testid="nft-search"]' },
      { url: '/governance', inputSelector: '[data-testid="proposal-search"]' },
    ];

    try {
      for (const testPage of testPages) {
        for (const payload of xssPayloads) {
          try {
            await page.goto(`${this.frontendUrl}${testPage.url}`);
            await page.waitForLoadState('networkidle');

            // Set up alert handler
            let alertTriggered = false;
            page.on('dialog', async dialog => {
              alertTriggered = true;
              await dialog.accept();
            });

            // Try to inject payload
            const inputElement = await page.$(testPage.inputSelector);
            if (inputElement) {
              await inputElement.fill(payload);
              await page.keyboard.press('Enter');
              await page.waitForTimeout(2000);

              if (alertTriggered) {
                this.addFinding({
                  type: 'xss',
                  category: 'reflected_xss',
                  severity: 'high',
                  url: `${this.frontendUrl}${testPage.url}`,
                  description: 'Reflected XSS vulnerability detected',
                  evidence: `XSS payload executed: ${payload}`,
                  recommendation:
                    'Implement proper input sanitization and output encoding',
                });
              }

              // Check for payload in DOM
              const bodyHTML = await page.innerHTML('body');
              if (bodyHTML.includes(payload)) {
                this.addFinding({
                  type: 'xss',
                  category: 'stored_xss_potential',
                  severity: 'medium',
                  url: `${this.frontendUrl}${testPage.url}`,
                  description:
                    'Potential stored XSS - payload reflected in DOM',
                  evidence: `Payload found in DOM: ${payload}`,
                  recommendation:
                    'Implement output encoding and Content Security Policy',
                });
              }
            }
          } catch (error) {
            console.warn(
              `⚠️  XSS test failed for ${testPage.url}: ${error.message}`
            );
          }
        }
      }
    } finally {
      await browser.close();
    }
  }

  /**
   * Scan CSRF protection
   */
  async scanCSRFProtection() {
    const csrfTests = [
      {
        name: 'POST without CSRF token',
        endpoint: '/api/wallet/transfer',
        method: 'POST',
        payload: { to: 'test-address', amount: '1.0' },
      },
      {
        name: 'PUT without CSRF token',
        endpoint: '/api/user/profile',
        method: 'PUT',
        payload: { name: 'Modified Name' },
      },
    ];

    for (const test of csrfTests) {
      try {
        const config = {
          method: test.method.toLowerCase(),
          url: `${this.baseUrl}${test.endpoint}`,
          data: test.payload,
          headers: {
            Origin: 'http://malicious-site.com',
            Referer: 'http://malicious-site.com',
          },
          timeout: 10000,
          validateStatus: () => true,
        };

        const response = await axios(config);

        // CSRF protection should reject cross-origin requests
        if (response.status === 200) {
          this.addFinding({
            type: 'csrf',
            category: 'csrf_missing',
            severity: 'high',
            url: `${this.baseUrl}${test.endpoint}`,
            description: `${test.name} - CSRF protection missing`,
            evidence: 'Cross-origin request succeeded without CSRF token',
            recommendation: 'Implement CSRF tokens and origin validation',
          });
        }
      } catch (error) {
        console.warn(`⚠️  CSRF test failed for ${test.name}: ${error.message}`);
      }
    }
  }

  /**
   * Add finding to results
   */
  addFinding(finding) {
    this.findings.push(finding);
    this.scanResults.findings.push(finding);

    // Update counters
    this.scanResults.summary.totalFindings++;
    switch (finding.severity) {
      case 'critical':
        this.scanResults.summary.criticalCount++;
        break;
      case 'high':
        this.scanResults.summary.highCount++;
        break;
      case 'medium':
        this.scanResults.summary.mediumCount++;
        break;
      case 'low':
        this.scanResults.summary.lowCount++;
        break;
    }
  }

  /**
   * Get header severity based on importance
   */
  getHeaderSeverity(header) {
    const criticalHeaders = [
      'Content-Security-Policy',
      'Strict-Transport-Security',
    ];
    const highHeaders = ['X-Frame-Options', 'X-Content-Type-Options'];

    if (criticalHeaders.includes(header)) return 'high';
    if (highHeaders.includes(header)) return 'medium';
    return 'low';
  }

  /**
   * Get header-specific recommendations
   */
  getHeaderRecommendation(header) {
    const recommendations = {
      'Content-Security-Policy': 'Implement CSP to prevent XSS attacks',
      'X-Content-Type-Options': 'Add "nosniff" to prevent MIME type sniffing',
      'X-Frame-Options': 'Add "DENY" or "SAMEORIGIN" to prevent clickjacking',
      'X-XSS-Protection': 'Add "1; mode=block" for legacy XSS protection',
      'Strict-Transport-Security': 'Enforce HTTPS with HSTS header',
      'Referrer-Policy': 'Control referrer information leakage',
      'Permissions-Policy': 'Control browser feature access',
    };

    return recommendations[header] || `Implement ${header} security header`;
  }

  /**
   * Generate JSON report
   */
  async generateJSONReport() {
    const reportPath = path.join(this.reportDir, 'dast-scan-results.json');
    fs.writeFileSync(reportPath, JSON.stringify(this.scanResults, null, 2));
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
    <title>DAST Security Scan Report</title>
    <style>
        body { font-family: Arial, sans-serif; margin: 20px; background: #f5f5f5; }
        .header { background: #1976d2; color: white; padding: 20px; border-radius: 8px; margin-bottom: 20px; }
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
        .evidence { background: #f5f5f5; padding: 10px; border-radius: 4px; margin: 10px 0; }
        .recommendation { background: #e8f5e8; padding: 10px; border-radius: 4px; margin-top: 10px; }
    </style>
</head>
<body>
    <div class="header">
        <h1>🔍 DAST Security Scan Report</h1>
        <p>Generated: ${this.scanResults.metadata.timestamp}</p>
        <p>Targets: ${this.scanResults.metadata.baseUrl} | ${this.scanResults.metadata.frontendUrl}</p>
    </div>

    <div class="summary">
        <h2>Scan Summary</h2>
        <div class="summary-grid">
            <div class="summary-card critical">
                <h3>${this.scanResults.summary.criticalCount}</h3>
                <p>Critical</p>
            </div>
            <div class="summary-card high">
                <h3>${this.scanResults.summary.highCount}</h3>
                <p>High</p>
            </div>
            <div class="summary-card medium">
                <h3>${this.scanResults.summary.mediumCount}</h3>
                <p>Medium</p>
            </div>
            <div class="summary-card low">
                <h3>${this.scanResults.summary.lowCount}</h3>
                <p>Low</p>
            </div>
        </div>
        <p><strong>Total Findings:</strong> ${this.scanResults.summary.totalFindings}</p>
    </div>

    <h2>Security Findings</h2>
    ${this.scanResults.findings
      .map(
        finding => `
        <div class="finding ${finding.severity}">
            <div class="finding-header">
                <h3>${finding.description}</h3>
                <span class="severity-badge" style="background: ${this.getSeverityColor(finding.severity)}">${finding.severity.toUpperCase()}</span>
            </div>
            <p><strong>URL:</strong> ${finding.url}</p>
            <p><strong>Type:</strong> ${finding.type} - ${finding.category}</p>
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

    const htmlPath = path.join(this.reportDir, 'dast-scan-report.html');
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
   * Print scan summary
   */
  printScanSummary() {
    console.log('\n' + '='.repeat(60));
    console.log('🔍 DAST SECURITY SCAN SUMMARY');
    console.log('='.repeat(60));
    console.log(`📊 Total Findings: ${this.scanResults.summary.totalFindings}`);
    console.log(`🚨 Critical: ${this.scanResults.summary.criticalCount}`);
    console.log(`⚠️  High: ${this.scanResults.summary.highCount}`);
    console.log(`⚡ Medium: ${this.scanResults.summary.mediumCount}`);
    console.log(`📝 Low: ${this.scanResults.summary.lowCount}`);

    if (this.scanResults.summary.criticalCount > 0) {
      console.log(
        '\n🚨 CRITICAL VULNERABILITIES FOUND - Immediate action required!'
      );
    } else if (this.scanResults.summary.highCount > 0) {
      console.log(
        '\n⚠️  HIGH SEVERITY VULNERABILITIES - Review and fix promptly'
      );
    } else {
      console.log('\n✅ No critical or high severity vulnerabilities found');
    }

    console.log('='.repeat(60));
  }
}

// Export for use in other test files
module.exports = {
  DASTScanner,
};

// Main execution function
async function runDASTScan(options = {}) {
  const scanner = new DASTScanner();
  return await scanner.runComprehensiveScan(options);
}

// Run scan if called directly
if (require.main === module) {
  runDASTScan({
    scanTypes: [
      'headers',
      'authentication',
      'authorization',
      'injection',
      'xss',
      'csrf',
    ],
    targets: ['api', 'frontend'],
  })
    .then(results => {
      const hasHighSeverity =
        results.summary.criticalCount > 0 || results.summary.highCount > 0;
      process.exit(hasHighSeverity ? 1 : 0);
    })
    .catch(error => {
      console.error('DAST scan execution failed:', error);
      process.exit(1);
    });
}
