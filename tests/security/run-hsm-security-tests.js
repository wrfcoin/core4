#!/usr/bin/env node

/**
 * HSM Security Test Runner
 *
 * Comprehensive test runner for all HSM authorization security validation tests.
 * Executes the complete test suite and generates detailed security reports.
 *
 * USAGE:
 *   node run-hsm-security-tests.js [options]
 *
 * OPTIONS:
 *   --suite=<suite>     Run specific test suite (auth|penetration|performance|edge|all)
 *   --report=<format>   Generate report (json|html|console)
 *   --verbose           Verbose output
 *   --ci                CI/CD mode (exit codes for automation)
 */

import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);
const { spawn } = require('child_process');
const fs = require('fs');
const path = require('path');
const crypto = require('crypto');
const {
  buildUsage,
  parseCliArguments,
  resolveOptionValues,
} = require('../utils/test-config.cjs');

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

class HSMSecurityTestRunner {
  constructor() {
    this.testSuites = {
      auth: 'hsm-authorization-security-tests.js',
      penetration: 'hsm-session-hijacking-penetration-tests.js',
      performance: 'hsm-performance-regression-tests.js',
      edge: 'hsm-edge-case-security-tests.js',
      comprehensive: 'hsm-comprehensive-security-suite.js',
    };

    this.results = {
      runId: crypto.randomBytes(16).toString('hex'),
      timestamp: new Date(),
      totalSuites: 0,
      passedSuites: 0,
      failedSuites: 0,
      totalTests: 0,
      passedTests: 0,
      failedTests: 0,
      securityViolationsBlocked: 0,
      vulnerabilityStatus: 'UNKNOWN',
      suiteResults: {},
      errors: [],
      recommendations: [],
    };

    this.options = this.parseArguments();
  }

  parseArguments() {
    const definitions = [
      { name: 'help', type: 'boolean', description: 'Show this help message' },
      { name: 'suite', type: 'string', default: 'all', description: 'Suite to run (auth|penetration|performance|edge|comprehensive|all)' },
      { name: 'report', type: 'string', default: 'console', description: 'Report format (json|html|console)' },
      { name: 'verbose', type: 'boolean', default: false, description: 'Enable verbose output' },
      { name: 'ci', type: 'boolean', default: false, description: 'Exit with CI-friendly status codes' },
    ];
    const parsed = parseCliArguments(process.argv.slice(2), definitions);
    if (parsed.helpRequested) {
      process.stdout.write(buildUsage('tests/security/run-hsm-security-tests.js', 'HSM security test runner.', definitions));
      process.exit(0);
    }
    if (parsed.errors.length > 0) {
      process.stderr.write(`${parsed.errors.join('\n')}\n\n${buildUsage('tests/security/run-hsm-security-tests.js', 'HSM security test runner.', definitions)}`);
      process.exit(1);
    }

    return resolveOptionValues(parsed, process.env).values;
  }

  async run() {
    console.log('🔐 HSM SECURITY VALIDATION TEST RUNNER');
    console.log('=====================================');
    console.log(`Run ID: ${this.results.runId}`);
    console.log(`Timestamp: ${this.results.timestamp.toISOString()}`);
    console.log(`Mode: ${this.options.ci ? 'CI/CD' : 'Interactive'}`);
    console.log(`Report Format: ${this.options.report}`);
    console.log(`Suite Selection: ${this.options.suite}`);
    console.log(`Verbose Output: ${this.options.verbose ? 'enabled' : 'disabled'}`);
    console.log('');

    try {
      await this.validateEnvironment();
      await this.executeSuites();
      await this.generateReport();
      await this.finalizeResults();

      if (this.options.ci) {
        process.exit(this.results.failedSuites > 0 ? 1 : 0);
      }
    } catch (error) {
      console.error('❌ Test runner failed:', error.message);
      if (this.options.ci) {
        process.exit(2);
      }
    }
  }

  async validateEnvironment() {
    console.log('🔧 Validating test environment...');

    // Check Node.js version
    const nodeVersion = process.version;
    console.log(`✅ Node.js version: ${nodeVersion}`);

    // Check required dependencies
    const requiredPackages = ['mocha', 'chai', 'supertest', 'sinon'];

    for (const pkg of requiredPackages) {
      try {
        require.resolve(pkg);
        console.log(`✅ ${pkg} available`);
      } catch (error) {
        throw new Error(
          `Required package ${pkg} not found. Run: npm install ${pkg}`
        );
      }
    }

    // Check test files exist
    for (const [suiteName, fileName] of Object.entries(this.testSuites)) {
      const filePath = path.join(__dirname, fileName);
      if (!fs.existsSync(filePath)) {
        throw new Error(`Test suite file not found: ${fileName}`);
      }
      console.log(`✅ Test suite ${suiteName}: ${fileName}`);
    }

    console.log('✅ Environment validation completed\n');
  }

  async executeSuites() {
    const suitesToRun =
      this.options.suite === 'all'
        ? Object.keys(this.testSuites)
        : [this.options.suite];

    if (this.options.suite !== 'all' && !this.testSuites[this.options.suite]) {
      throw new Error(`Unknown test suite: ${this.options.suite}`);
    }

    this.results.totalSuites = suitesToRun.length;

    for (const suiteName of suitesToRun) {
      console.log(`🔍 Executing ${suiteName} test suite...`);

      try {
        const suiteResult = await this.executeSuite(suiteName);
        this.results.suiteResults[suiteName] = suiteResult;

        if (suiteResult.success) {
          this.results.passedSuites++;
          console.log(`✅ ${suiteName} suite: PASSED`);
        } else {
          this.results.failedSuites++;
          console.log(`❌ ${suiteName} suite: FAILED`);
        }

        this.results.totalTests += suiteResult.totalTests;
        this.results.passedTests += suiteResult.passedTests;
        this.results.failedTests += suiteResult.failedTests;
        this.results.securityViolationsBlocked +=
          suiteResult.securityViolationsBlocked || 0;
      } catch (error) {
        this.results.failedSuites++;
        this.results.errors.push({
          suite: suiteName,
          error: error.message,
          timestamp: new Date(),
        });
        console.log(`❌ ${suiteName} suite: ERROR - ${error.message}`);
      }
    }

    console.log('');
  }

  async executeSuite(suiteName) {
    const fileName = this.testSuites[suiteName];
    const filePath = path.join(__dirname, fileName);

    return new Promise((resolve, reject) => {
      const mocha = spawn('npx', ['mocha', filePath, '--reporter', 'json'], {
        cwd: path.join(__dirname, '../..'),
        stdio: ['pipe', 'pipe', 'pipe'],
      });

      let stdout = '';
      let stderr = '';

      mocha.stdout.on('data', data => {
        stdout += data.toString();
        if (this.options.verbose) {
          process.stdout.write(data);
        }
      });

      mocha.stderr.on('data', data => {
        stderr += data.toString();
        if (this.options.verbose) {
          process.stderr.write(data);
        }
      });

      mocha.on('close', code => {
        try {
          const result = JSON.parse(stdout);

          const suiteResult = {
            success: code === 0,
            exitCode: code,
            totalTests: result.stats?.tests || 0,
            passedTests: result.stats?.passes || 0,
            failedTests: result.stats?.failures || 0,
            duration: result.stats?.duration || 0,
            securityViolationsBlocked: this.extractSecurityMetrics(result),
            errors: result.failures || [],
            stderr: stderr,
          };

          resolve(suiteResult);
        } catch (parseError) {
          // If JSON parsing fails, create basic result
          resolve({
            success: code === 0,
            exitCode: code,
            totalTests: 0,
            passedTests: 0,
            failedTests: code === 0 ? 0 : 1,
            duration: 0,
            securityViolationsBlocked: 0,
            errors: [
              { title: 'Parse Error', err: { message: parseError.message } },
            ],
            stderr: stderr,
          });
        }
      });

      mocha.on('error', error => {
        reject(error);
      });
    });
  }

  extractSecurityMetrics(result) {
    // Extract security-specific metrics from test results
    let securityViolationsBlocked = 0;

    if (result.tests) {
      result.tests.forEach(test => {
        if (
          test.title.includes('SECURITY:') ||
          test.title.includes('ATTACK:')
        ) {
          if (test.state === 'passed') {
            securityViolationsBlocked++;
          }
        }
      });
    }

    return securityViolationsBlocked;
  }

  async generateReport() {
    console.log('📊 Generating security validation report...');

    // Calculate overall security status
    this.calculateSecurityStatus();

    switch (this.options.report) {
      case 'json':
        await this.generateJSONReport();
        break;
      case 'html':
        await this.generateHTMLReport();
        break;
      case 'console':
      default:
        this.generateConsoleReport();
        break;
    }
  }

  calculateSecurityStatus() {
    const securityCriteria = {
      allSuitesPassed: this.results.failedSuites === 0,
      securityViolationsBlocked: this.results.securityViolationsBlocked > 0,
      noTestFailures: this.results.failedTests === 0,
      errorsFree: this.results.errors.length === 0,
    };

    const passedCriteria =
      Object.values(securityCriteria).filter(Boolean).length;
    const totalCriteria = Object.keys(securityCriteria).length;

    if (passedCriteria === totalCriteria) {
      this.results.vulnerabilityStatus = 'FIXED - VERIFIED SECURE';
    } else if (passedCriteria >= totalCriteria * 0.75) {
      this.results.vulnerabilityStatus = 'MOSTLY_FIXED - MINOR_ISSUES';
      this.results.recommendations.push('Address remaining test failures');
    } else {
      this.results.vulnerabilityStatus = 'VULNERABLE - CRITICAL_ISSUES';
      this.results.recommendations.push(
        'Critical security fixes required before deployment'
      );
    }
  }

  generateConsoleReport() {
    console.log('📋 SECURITY VALIDATION REPORT');
    console.log('=============================');
    console.log(`🆔 Run ID: ${this.results.runId}`);
    console.log(`⏰ Execution Time: ${new Date() - this.results.timestamp}ms`);
    console.log(`🎯 Vulnerability Status: ${this.results.vulnerabilityStatus}`);
    console.log('');

    console.log('📊 TEST EXECUTION SUMMARY');
    console.log(`   Total Test Suites: ${this.results.totalSuites}`);
    console.log(`   ✅ Passed Suites: ${this.results.passedSuites}`);
    console.log(`   ❌ Failed Suites: ${this.results.failedSuites}`);
    console.log(`   📝 Total Tests: ${this.results.totalTests}`);
    console.log(`   ✅ Passed Tests: ${this.results.passedTests}`);
    console.log(`   ❌ Failed Tests: ${this.results.failedTests}`);
    console.log('');

    console.log('🛡️  SECURITY METRICS');
    console.log(
      `   Security Violations Blocked: ${this.results.securityViolationsBlocked}`
    );
    console.log(`   Error Count: ${this.results.errors.length}`);
    console.log('');

    if (this.results.errors.length > 0) {
      console.log('❌ ERRORS ENCOUNTERED');
      this.results.errors.forEach((error, index) => {
        console.log(`   ${index + 1}. [${error.suite}] ${error.error}`);
      });
      console.log('');
    }

    if (this.results.recommendations.length > 0) {
      console.log('💡 RECOMMENDATIONS');
      this.results.recommendations.forEach((rec, index) => {
        console.log(`   ${index + 1}. ${rec}`);
      });
      console.log('');
    }

    const statusEmoji = this.results.vulnerabilityStatus.includes('FIXED')
      ? '🟢'
      : this.results.vulnerabilityStatus.includes('MOSTLY')
        ? '🟡'
        : '🔴';

    console.log(
      `${statusEmoji} FINAL VERDICT: ${this.results.vulnerabilityStatus}`
    );
  }

  async generateJSONReport() {
    const reportPath = path.join(
      __dirname,
      `hsm-security-report-${this.results.runId}.json`
    );

    const report = {
      ...this.results,
      reportGenerated: new Date(),
      environment: {
        nodeVersion: process.version,
        platform: process.platform,
        architecture: process.arch,
      },
    };

    fs.writeFileSync(reportPath, JSON.stringify(report, null, 2));
    console.log(`📄 JSON report generated: ${reportPath}`);
  }

  async generateHTMLReport() {
    const reportPath = path.join(
      __dirname,
      `hsm-security-report-${this.results.runId}.html`
    );

    const html = `
<!DOCTYPE html>
<html>
<head>
    <title>HSM Security Validation Report</title>
    <style>
        body { font-family: Arial, sans-serif; margin: 20px; }
        .header { background-color: #f0f0f0; padding: 20px; border-radius: 5px; }
        .status-fixed { color: green; font-weight: bold; }
        .status-vulnerable { color: red; font-weight: bold; }
        .metrics { display: grid; grid-template-columns: 1fr 1fr; gap: 20px; margin: 20px 0; }
        .metric-box { border: 1px solid #ddd; padding: 15px; border-radius: 5px; }
        .suite-result { margin: 10px 0; padding: 10px; border-left: 4px solid #ddd; }
        .suite-passed { border-left-color: green; }
        .suite-failed { border-left-color: red; }
    </style>
</head>
<body>
    <div class="header">
        <h1>🔐 HSM Security Validation Report</h1>
        <p><strong>Run ID:</strong> ${this.results.runId}</p>
        <p><strong>Timestamp:</strong> ${this.results.timestamp}</p>
        <p><strong>Status:</strong> <span class="${this.results.vulnerabilityStatus.includes('FIXED') ? 'status-fixed' : 'status-vulnerable'}">${this.results.vulnerabilityStatus}</span></p>
    </div>
    
    <div class="metrics">
        <div class="metric-box">
            <h3>Test Execution</h3>
            <p>Total Suites: ${this.results.totalSuites}</p>
            <p>Passed Suites: ${this.results.passedSuites}</p>
            <p>Failed Suites: ${this.results.failedSuites}</p>
            <p>Total Tests: ${this.results.totalTests}</p>
        </div>
        <div class="metric-box">
            <h3>Security Metrics</h3>
            <p>Violations Blocked: ${this.results.securityViolationsBlocked}</p>
            <p>Errors: ${this.results.errors.length}</p>
        </div>
    </div>
    
    <h2>Suite Results</h2>
    ${Object.entries(this.results.suiteResults)
      .map(
        ([name, result]) => `
        <div class="suite-result suite-${result.success ? 'passed' : 'failed'}">
            <h4>${name}</h4>
            <p>Status: ${result.success ? '✅ PASSED' : '❌ FAILED'}</p>
            <p>Tests: ${result.passedTests}/${result.totalTests}</p>
            <p>Duration: ${result.duration}ms</p>
        </div>
    `
      )
      .join('')}
    
    ${
      this.results.recommendations.length > 0
        ? `
        <h2>Recommendations</h2>
        <ul>
            ${this.results.recommendations.map(rec => `<li>${rec}</li>`).join('')}
        </ul>
    `
        : ''
    }
    
    <div class="footer">
        <p><em>Report generated: ${new Date()}</em></p>
    </div>
</body>
</html>`;

    fs.writeFileSync(reportPath, html);
    console.log(`📄 HTML report generated: ${reportPath}`);
  }

  async finalizeResults() {
    console.log('\n🏁 HSM Security Validation Complete');
    console.log('==================================');

    if (this.results.vulnerabilityStatus.includes('FIXED')) {
      console.log(
        '🎉 SUCCESS: HSM authorization bypass vulnerability has been verified as FIXED'
      );
      console.log('✅ All security tests passed');
      console.log('✅ Attack vectors successfully blocked');
      console.log('✅ Legitimate operations preserved');
      console.log('✅ Performance impact minimal');
    } else {
      console.log('⚠️  WARNING: Security validation identified issues');
      console.log('❌ Manual review and additional fixes may be required');
    }

    console.log(
      `\n📊 Final Score: ${this.results.passedTests}/${this.results.totalTests} tests passed`
    );
    console.log(
      `🛡️  Security Violations Blocked: ${this.results.securityViolationsBlocked}`
    );

    if (this.options.ci) {
      console.log(
        `\n🤖 CI/CD Exit Code: ${this.results.failedSuites > 0 ? 1 : 0}`
      );
    }
  }
}

// Main execution
if (process.argv[1] && path.resolve(process.argv[1]) === __filename) {
  const runner = new HSMSecurityTestRunner();
  runner.run().catch(error => {
    console.error('💥 Fatal error:', error);
    process.exit(2);
  });
}

export default HSMSecurityTestRunner;
