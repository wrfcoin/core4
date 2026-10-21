/**
 * Comprehensive Security Test Runner
 * Orchestrates all security testing tools and generates unified reports
 */

import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);
const fs = require('fs');
const path = require('path');
const { performance } = require('perf_hooks');
const {
  buildUsage,
  parseCliArguments,
  resolveOptionValues,
} = require('../utils/test-config.cjs');

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

class ComprehensiveSecurityRunner {
  constructor() {
    this.reportDir = path.join(__dirname, '../reports/security');
    this.startTime = null;
    this.results = {
      sast: null,
      dast: null,
      penetration: null,
    };

    // Ensure report directory exists
    if (!fs.existsSync(this.reportDir)) {
      fs.mkdirSync(this.reportDir, { recursive: true });
    }
  }

  /**
   * Run all security test suites
   */
  async runAllSecurityTests(options = {}) {
    const {
      suites = ['sast', 'dast', 'penetration'],
      parallel = false,
      generateReport = true,
      severity = 'low',
    } = options;

    console.log('🔒 Starting Comprehensive WRFCoin Security Testing\n');
    console.log(`🛡️  Test Suites: ${suites.join(', ')}`);
    console.log(`⚡ Execution Mode: ${parallel ? 'Parallel' : 'Sequential'}\n`);

    this.startTime = performance.now();

    try {
      if (parallel) {
        await this.runTestsInParallel(suites, severity);
      } else {
        await this.runTestsSequentially(suites, severity);
      }

      if (generateReport) {
        return await this.generateUnifiedReport();
      }

      return this.results;
    } catch (error) {
      console.error('❌ Security testing failed:', error);
      throw error;
    }
  }

  /**
   * Run test suites in parallel
   */
  async runTestsInParallel(suites, severity) {
    const testPromises = [];

    if (suites.includes('sast')) {
      testPromises.push(
        this.runSAST(severity)
          .then(result => {
            this.results.sast = result;
            console.log('✅ SAST Security Scan completed');
          })
          .catch(error => {
            console.error('❌ SAST Security Scan failed:', error.message);
            this.results.sast = { error: error.message };
          })
      );
    }

    if (suites.includes('dast')) {
      testPromises.push(
        this.runDAST()
          .then(result => {
            this.results.dast = result;
            console.log('✅ DAST Security Scan completed');
          })
          .catch(error => {
            console.error('❌ DAST Security Scan failed:', error.message);
            this.results.dast = { error: error.message };
          })
      );
    }

    if (suites.includes('penetration')) {
      testPromises.push(
        this.runPenetrationTests()
          .then(result => {
            this.results.penetration = result;
            console.log('✅ Penetration Testing completed');
          })
          .catch(error => {
            console.error('❌ Penetration Testing failed:', error.message);
            this.results.penetration = { error: error.message };
          })
      );
    }

    await Promise.all(testPromises);
  }

  /**
   * Run test suites sequentially
   */
  async runTestsSequentially(suites, severity) {
    for (const suite of suites) {
      try {
        console.log(`\n🔍 Running ${suite.toUpperCase()} Security Tests...`);

        switch (suite) {
          case 'sast':
            this.results.sast = await this.runSAST(severity);
            break;
          case 'dast':
            this.results.dast = await this.runDAST();
            break;
          case 'penetration':
            this.results.penetration = await this.runPenetrationTests();
            break;
          default:
            console.warn(`⚠️  Unknown test suite: ${suite}`);
        }

        console.log(`✅ ${suite.toUpperCase()} Security Tests completed`);
      } catch (error) {
        console.error(
          `❌ ${suite.toUpperCase()} Security Tests failed:`,
          error.message
        );
        this.results[suite] = { error: error.message };
      }
    }
  }

  /**
   * Run SAST security scan
   */
  async runSAST(severity = 'low') {
    const { SASTScanner } = require('./sast-scanner');
    const scanner = new SASTScanner();
    return await scanner.runComprehensiveScan({
      scanTypes: ['secrets', 'vulnerabilities', 'dependencies', 'config'],
      outputFormats: ['json'],
      severity,
    });
  }

  /**
   * Run DAST security scan
   */
  async runDAST() {
    const { DASTScanner } = require('./dast-scanner');
    const scanner = new DASTScanner();
    return await scanner.runComprehensiveScan({
      scanTypes: [
        'headers',
        'authentication',
        'authorization',
        'injection',
        'xss',
        'csrf',
      ],
      targets: ['api', 'frontend'],
    });
  }

  /**
   * Run penetration tests
   */
  async runPenetrationTests() {
    const { PenetrationTester } = require('./penetration-tests');
    const tester = new PenetrationTester();
    return await tester.runPenetrationTests({
      testSuites: [
        'authentication',
        'authorization',
        'session',
        'business_logic',
        'data_exposure',
      ],
      setupMode: 'mock',
    });
  }

  /**
   * Generate unified security report
   */
  async generateUnifiedReport() {
    const endTime = performance.now();
    const totalDuration = (endTime - this.startTime) / 1000;

    const unifiedReport = {
      metadata: {
        timestamp: new Date().toISOString(),
        duration: Math.round(totalDuration),
        platform: process.platform,
        nodeVersion: process.version,
        testRunner: 'WRFCoin Comprehensive Security Suite',
      },
      summary: {
        overallStatus: this.calculateOverallSecurityStatus(),
        security: {
          sast: this.summarizeSAST(),
          dast: this.summarizeDAST(),
          penetration: this.summarizePenetration(),
        },
        criticalIssues: this.identifyCriticalSecurityIssues(),
        recommendations: this.generateUnifiedSecurityRecommendations(),
        complianceStatus: this.assessComplianceStatus(),
      },
      detailed: this.results,
    };

    // Save unified report
    const reportPath = path.join(
      this.reportDir,
      'unified-security-report.json'
    );
    fs.writeFileSync(reportPath, JSON.stringify(unifiedReport, null, 2));

    // Generate HTML dashboard
    await this.generateHTMLSecurityDashboard(unifiedReport);

    // Print summary
    this.printUnifiedSecuritySummary(unifiedReport);

    return unifiedReport;
  }

  /**
   * Calculate overall security status
   */
  calculateOverallSecurityStatus() {
    const statuses = [];

    if (this.results.sast && !this.results.sast.error) {
      const criticalCount = this.results.sast.summary?.criticalCount || 0;
      const highCount = this.results.sast.summary?.highCount || 0;
      statuses.push(criticalCount === 0 && highCount === 0 ? 'PASS' : 'FAIL');
    }

    if (this.results.dast && !this.results.dast.error) {
      const criticalCount = this.results.dast.summary?.criticalCount || 0;
      const highCount = this.results.dast.summary?.highCount || 0;
      statuses.push(criticalCount === 0 && highCount === 0 ? 'PASS' : 'FAIL');
    }

    if (this.results.penetration && !this.results.penetration.error) {
      const criticalFindings =
        this.results.penetration.summary?.criticalFindings || 0;
      const highFindings = this.results.penetration.summary?.highFindings || 0;
      statuses.push(
        criticalFindings === 0 && highFindings === 0 ? 'PASS' : 'FAIL'
      );
    }

    const passCount = statuses.filter(s => s === 'PASS').length;
    const totalCount = statuses.length;

    if (totalCount === 0) return 'ERROR';
    if (passCount === totalCount) return 'SECURE';
    if (passCount >= totalCount * 0.8) return 'GOOD';
    if (passCount >= totalCount * 0.6) return 'ACCEPTABLE';
    return 'VULNERABLE';
  }

  /**
   * Summarize SAST results
   */
  summarizeSAST() {
    if (!this.results.sast || this.results.sast.error) {
      return {
        status: 'ERROR',
        message: this.results.sast?.error || 'No data',
      };
    }

    return {
      status:
        this.results.sast.summary?.criticalCount === 0 &&
        this.results.sast.summary?.highCount === 0
          ? 'PASS'
          : 'FAIL',
      totalVulnerabilities:
        this.results.sast.summary?.totalVulnerabilities || 0,
      criticalCount: this.results.sast.summary?.criticalCount || 0,
      highCount: this.results.sast.summary?.highCount || 0,
      mediumCount: this.results.sast.summary?.mediumCount || 0,
      lowCount: this.results.sast.summary?.lowCount || 0,
    };
  }

  /**
   * Summarize DAST results
   */
  summarizeDAST() {
    if (!this.results.dast || this.results.dast.error) {
      return {
        status: 'ERROR',
        message: this.results.dast?.error || 'No data',
      };
    }

    return {
      status:
        this.results.dast.summary?.criticalCount === 0 &&
        this.results.dast.summary?.highCount === 0
          ? 'PASS'
          : 'FAIL',
      totalFindings: this.results.dast.summary?.totalFindings || 0,
      criticalCount: this.results.dast.summary?.criticalCount || 0,
      highCount: this.results.dast.summary?.highCount || 0,
      mediumCount: this.results.dast.summary?.mediumCount || 0,
      lowCount: this.results.dast.summary?.lowCount || 0,
    };
  }

  /**
   * Summarize penetration test results
   */
  summarizePenetration() {
    if (!this.results.penetration || this.results.penetration.error) {
      return {
        status: 'ERROR',
        message: this.results.penetration?.error || 'No data',
      };
    }

    return {
      status:
        this.results.penetration.summary?.criticalFindings === 0 &&
        this.results.penetration.summary?.highFindings === 0
          ? 'PASS'
          : 'FAIL',
      totalTests: this.results.penetration.summary?.totalTests || 0,
      successful: this.results.penetration.summary?.successful || 0,
      failed: this.results.penetration.summary?.failed || 0,
      criticalFindings: this.results.penetration.summary?.criticalFindings || 0,
      highFindings: this.results.penetration.summary?.highFindings || 0,
    };
  }

  /**
   * Identify critical security issues across all tests
   */
  identifyCriticalSecurityIssues() {
    const issues = [];

    // SAST critical issues
    if (this.results.sast?.vulnerabilities) {
      this.results.sast.vulnerabilities.forEach(vuln => {
        if (vuln.severity === 'critical' || vuln.severity === 'high') {
          issues.push({
            severity: vuln.severity,
            source: 'SAST',
            type: vuln.category,
            description: vuln.description,
            file: vuln.file,
            recommendation: vuln.recommendation,
          });
        }
      });
    }

    // DAST critical issues
    if (this.results.dast?.findings) {
      this.results.dast.findings.forEach(finding => {
        if (finding.severity === 'critical' || finding.severity === 'high') {
          issues.push({
            severity: finding.severity,
            source: 'DAST',
            type: finding.category,
            description: finding.description,
            url: finding.url,
            recommendation: finding.recommendation,
          });
        }
      });
    }

    // Penetration test critical issues
    if (this.results.penetration?.findings) {
      this.results.penetration.findings.forEach(finding => {
        if (finding.severity === 'critical' || finding.severity === 'high') {
          issues.push({
            severity: finding.severity,
            source: 'Penetration',
            type: finding.type,
            description: finding.testName,
            evidence: finding.evidence,
            recommendation: finding.recommendation,
          });
        }
      });
    }

    return issues.sort((a, b) => {
      const severityOrder = { critical: 4, high: 3, medium: 2, low: 1 };
      return severityOrder[b.severity] - severityOrder[a.severity];
    });
  }

  /**
   * Generate unified security recommendations
   */
  generateUnifiedSecurityRecommendations() {
    const recommendations = [];

    // Collect high-priority recommendations from each test suite
    if (this.results.sast?.vulnerabilities) {
      const highSeverityVulns = this.results.sast.vulnerabilities.filter(
        v => v.severity === 'critical' || v.severity === 'high'
      );
      highSeverityVulns.slice(0, 5).forEach(vuln => {
        recommendations.push({
          priority: 'High',
          category: 'Static Code Analysis',
          issue: vuln.description,
          action: vuln.recommendation,
          impact: 'Security vulnerability in source code',
        });
      });
    }

    if (this.results.dast?.findings) {
      const highSeverityFindings = this.results.dast.findings.filter(
        f => f.severity === 'critical' || f.severity === 'high'
      );
      highSeverityFindings.slice(0, 5).forEach(finding => {
        recommendations.push({
          priority: 'High',
          category: 'Runtime Security',
          issue: finding.description,
          action: finding.recommendation,
          impact: 'Runtime security vulnerability',
        });
      });
    }

    if (this.results.penetration?.findings) {
      const highSeverityFindings = this.results.penetration.findings.filter(
        f => f.severity === 'critical' || f.severity === 'high'
      );
      highSeverityFindings.slice(0, 5).forEach(finding => {
        recommendations.push({
          priority: 'High',
          category: 'Penetration Testing',
          issue: finding.testName,
          action: finding.recommendation,
          impact: 'Security control bypass',
        });
      });
    }

    return recommendations.slice(0, 15); // Limit to top 15 recommendations
  }

  /**
   * Assess compliance status
   */
  assessComplianceStatus() {
    const compliance = {
      OWASP_Top_10: 'PARTIAL',
      Data_Protection: 'NEEDS_REVIEW',
      Authentication: 'NEEDS_IMPROVEMENT',
      Authorization: 'NEEDS_IMPROVEMENT',
      Encryption: 'NEEDS_REVIEW',
    };

    // Update based on test results
    if (this.results.dast?.findings) {
      const authFindings = this.results.dast.findings.filter(
        f => f.type === 'authentication'
      );
      if (authFindings.length === 0) {
        compliance.Authentication = 'COMPLIANT';
      }

      const authzFindings = this.results.dast.findings.filter(
        f => f.type === 'authorization'
      );
      if (authzFindings.length === 0) {
        compliance.Authorization = 'COMPLIANT';
      }
    }

    return compliance;
  }

  /**
   * Generate HTML security dashboard
   */
  async generateHTMLSecurityDashboard(reportData) {
    const htmlTemplate = `
<!DOCTYPE html>
<html>
<head>
    <title>WRFCoin Security Dashboard</title>
    <style>
        body { font-family: Arial, sans-serif; margin: 0; padding: 20px; background: #f5f5f5; }
        .header { background: #d32f2f; color: white; padding: 20px; border-radius: 8px; margin-bottom: 20px; }
        .summary-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(300px, 1fr)); gap: 20px; margin-bottom: 20px; }
        .summary-card { background: white; padding: 20px; border-radius: 8px; box-shadow: 0 2px 4px rgba(0,0,0,0.1); }
        .status-secure { border-left: 5px solid #4CAF50; }
        .status-good { border-left: 5px solid #8BC34A; }
        .status-acceptable { border-left: 5px solid #FF9800; }
        .status-vulnerable { border-left: 5px solid #f44336; }
        .status-error { border-left: 5px solid #9E9E9E; }
        .metric { display: flex; justify-content: space-between; margin: 10px 0; }
        .metric-value { font-weight: bold; }
        .issues-section { background: white; padding: 20px; border-radius: 8px; margin: 20px 0; }
        .issue-critical { background: #ffebee; border-left: 4px solid #f44336; padding: 10px; margin: 5px 0; }
        .issue-high { background: #fff3e0; border-left: 4px solid #ff9800; padding: 10px; margin: 5px 0; }
        .recommendations { background: white; padding: 20px; border-radius: 8px; margin: 20px 0; }
        .recommendation { background: #e8f5e8; border-left: 4px solid #4caf50; padding: 15px; margin: 10px 0; border-radius: 4px; }
        .compliance-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 10px; }
        .compliance-item { padding: 10px; border-radius: 4px; text-align: center; }
        .compliant { background: #e8f5e8; color: #2e7d32; }
        .partial { background: #fff3e0; color: #f57c00; }
        .needs-review { background: #ffebee; color: #d32f2f; }
        .timestamp { color: #666; font-size: 0.9em; }
    </style>
</head>
<body>
    <div class="header">
        <h1>🔒 WRFCoin Security Dashboard</h1>
        <p class="timestamp">Generated: ${reportData.metadata.timestamp}</p>
        <p>Test Duration: ${reportData.metadata.duration}s | Overall Status: <strong>${reportData.summary.overallStatus}</strong></p>
    </div>

    <div class="summary-grid">
        <div class="summary-card status-${reportData.summary.security.sast.status.toLowerCase()}">
            <h3>🔍 Static Analysis (SAST)</h3>
            <div class="metric">
                <span>Status:</span>
                <span class="metric-value">${reportData.summary.security.sast.status}</span>
            </div>
            <div class="metric">
                <span>Total Vulnerabilities:</span>
                <span class="metric-value">${reportData.summary.security.sast.totalVulnerabilities || 'N/A'}</span>
            </div>
            <div class="metric">
                <span>Critical:</span>
                <span class="metric-value">${reportData.summary.security.sast.criticalCount || 0}</span>
            </div>
            <div class="metric">
                <span>High:</span>
                <span class="metric-value">${reportData.summary.security.sast.highCount || 0}</span>
            </div>
        </div>

        <div class="summary-card status-${reportData.summary.security.dast.status.toLowerCase()}">
            <h3>🌐 Dynamic Analysis (DAST)</h3>
            <div class="metric">
                <span>Status:</span>
                <span class="metric-value">${reportData.summary.security.dast.status}</span>
            </div>
            <div class="metric">
                <span>Total Findings:</span>
                <span class="metric-value">${reportData.summary.security.dast.totalFindings || 'N/A'}</span>
            </div>
            <div class="metric">
                <span>Critical:</span>
                <span class="metric-value">${reportData.summary.security.dast.criticalCount || 0}</span>
            </div>
            <div class="metric">
                <span>High:</span>
                <span class="metric-value">${reportData.summary.security.dast.highCount || 0}</span>
            </div>
        </div>

        <div class="summary-card status-${reportData.summary.security.penetration.status.toLowerCase()}">
            <h3>🎯 Penetration Testing</h3>
            <div class="metric">
                <span>Status:</span>
                <span class="metric-value">${reportData.summary.security.penetration.status}</span>
            </div>
            <div class="metric">
                <span>Tests Run:</span>
                <span class="metric-value">${reportData.summary.security.penetration.totalTests || 'N/A'}</span>
            </div>
            <div class="metric">
                <span>Critical Findings:</span>
                <span class="metric-value">${reportData.summary.security.penetration.criticalFindings || 0}</span>
            </div>
            <div class="metric">
                <span>High Findings:</span>
                <span class="metric-value">${reportData.summary.security.penetration.highFindings || 0}</span>
            </div>
        </div>
    </div>

    ${
      reportData.summary.criticalIssues.length > 0
        ? `
        <div class="issues-section">
            <h2>🚨 Critical Security Issues (${reportData.summary.criticalIssues.length})</h2>
            ${reportData.summary.criticalIssues
              .slice(0, 10)
              .map(
                issue => `
                <div class="issue-${issue.severity}">
                    <strong>${issue.source} - ${issue.severity.toUpperCase()}</strong><br>
                    ${issue.description}<br>
                    <em>Recommendation: ${issue.recommendation}</em>
                </div>
            `
              )
              .join('')}
            ${reportData.summary.criticalIssues.length > 10 ? `<p><em>... and ${reportData.summary.criticalIssues.length - 10} more issues</em></p>` : ''}
        </div>
    `
        : ''
    }

    <div class="issues-section">
        <h2>📋 Compliance Status</h2>
        <div class="compliance-grid">
            ${Object.entries(reportData.summary.complianceStatus)
              .map(
                ([standard, status]) => `
                <div class="compliance-item ${status.toLowerCase().replace('_', '-')}">
                    <strong>${standard.replace('_', ' ')}</strong><br>
                    ${status.replace('_', ' ')}
                </div>
            `
              )
              .join('')}
        </div>
    </div>

    <div class="recommendations">
        <h2>💡 Security Recommendations (${reportData.summary.recommendations.length})</h2>
        ${reportData.summary.recommendations
          .slice(0, 10)
          .map(
            rec => `
            <div class="recommendation">
                <strong>${rec.category} - ${rec.priority} Priority</strong><br>
                <strong>Issue:</strong> ${rec.issue}<br>
                <strong>Action:</strong> ${rec.action}<br>
                <strong>Impact:</strong> ${rec.impact}
            </div>
        `
          )
          .join('')}
        ${reportData.summary.recommendations.length > 10 ? `<p><em>... and ${reportData.summary.recommendations.length - 10} more recommendations</em></p>` : ''}
    </div>
</body>
</html>`;

    const htmlPath = path.join(this.reportDir, 'security-dashboard.html');
    fs.writeFileSync(htmlPath, htmlTemplate);

    console.log(`🛡️  Security dashboard generated: ${htmlPath}`);
  }

  /**
   * Print unified security summary
   */
  printUnifiedSecuritySummary(report) {
    console.log('\n' + '='.repeat(80));
    console.log('🔒 COMPREHENSIVE SECURITY TEST SUMMARY');
    console.log('='.repeat(80));

    console.log(
      `\n🛡️  Overall Security Status: ${report.summary.overallStatus}`
    );
    console.log(`⏱️  Total Duration: ${report.metadata.duration}s`);

    console.log('\n🔍 Security Test Results:');
    console.log(
      `   🔍 SAST: ${report.summary.security.sast.status} (${report.summary.security.sast.totalVulnerabilities || 0} vulnerabilities)`
    );
    console.log(
      `   🌐 DAST: ${report.summary.security.dast.status} (${report.summary.security.dast.totalFindings || 0} findings)`
    );
    console.log(
      `   🎯 Penetration: ${report.summary.security.penetration.status} (${report.summary.security.penetration.totalTests || 0} tests)`
    );

    if (report.summary.criticalIssues.length > 0) {
      console.log(
        `\n🚨 Critical Security Issues: ${report.summary.criticalIssues.length}`
      );
      report.summary.criticalIssues.slice(0, 3).forEach(issue => {
        console.log(
          `   ${issue.severity.toUpperCase()}: ${issue.source} - ${issue.description}`
        );
      });
      if (report.summary.criticalIssues.length > 3) {
        console.log(
          `   ... and ${report.summary.criticalIssues.length - 3} more issues`
        );
      }
    }

    if (report.summary.recommendations.length > 0) {
      console.log(`\n💡 Top Security Recommendations:`);
      report.summary.recommendations.slice(0, 3).forEach((rec, index) => {
        console.log(`   ${index + 1}. ${rec.category}: ${rec.action}`);
      });
    }

    console.log('\n📋 Detailed reports available in: test/reports/security/');
    console.log('='.repeat(80));
  }
}

// Main execution function
async function runComprehensiveSecurityTests(options = {}) {
  const runner = new ComprehensiveSecurityRunner();
  return await runner.runAllSecurityTests(options);
}

// Run tests if called directly
if (process.argv[1] && path.resolve(process.argv[1]) === __filename) {
  const definitions = [
    { name: 'help', type: 'boolean', description: 'Show this help message' },
    { name: 'sast-only', type: 'boolean', default: false, description: 'Run only SAST checks' },
    { name: 'dast-only', type: 'boolean', default: false, description: 'Run only DAST checks' },
    { name: 'pentest-only', type: 'boolean', default: false, description: 'Run only penetration checks' },
    { name: 'parallel', type: 'boolean', default: false, description: 'Run enabled suites in parallel' },
    { name: 'no-report', type: 'boolean', default: false, description: 'Skip report generation' },
    { name: 'critical-only', type: 'boolean', default: false, description: 'Limit output to critical findings' },
  ];
  const parsed = parseCliArguments(process.argv.slice(2), definitions);
  if (parsed.helpRequested) {
    process.stdout.write(buildUsage('tests/security/comprehensive-security-runner.js', 'Comprehensive security runner.', definitions));
    process.exit(0);
  }
  if (parsed.errors.length > 0) {
    process.stderr.write(`${parsed.errors.join('\n')}\n\n${buildUsage('tests/security/comprehensive-security-runner.js', 'Comprehensive security runner.', definitions)}`);
    process.exit(1);
  }

  const values = resolveOptionValues(parsed, process.env).values;
  const exclusiveSuites = ['sast-only', 'dast-only', 'pentest-only'].filter((name) => values[name]);
  if (exclusiveSuites.length > 1) {
    process.stderr.write('Choose at most one of --sast-only, --dast-only, or --pentest-only.\n');
    process.exit(1);
  }

  const options = {
    suites: values['sast-only']
      ? ['sast']
      : values['dast-only']
        ? ['dast']
        : values['pentest-only']
          ? ['penetration']
          : ['sast', 'dast', 'penetration'],
    parallel: values.parallel,
    generateReport: !values['no-report'],
    severity: values['critical-only'] ? 'critical' : 'low',
  };

  console.log(`ℹ️  Comprehensive security runner: suites=${options.suites.join(',')} parallel=${options.parallel} report=${options.generateReport ? 'enabled' : 'disabled'} severity=${options.severity}`);

  runComprehensiveSecurityTests(options)
    .then(report => {
      const status = report.summary?.overallStatus;
      process.exit(['SECURE', 'GOOD'].includes(status) ? 0 : 1);
    })
    .catch(error => {
      console.error('Comprehensive security testing failed:', error);
      process.exit(1);
    });
}

export {
  ComprehensiveSecurityRunner,
  runComprehensiveSecurityTests,
};
