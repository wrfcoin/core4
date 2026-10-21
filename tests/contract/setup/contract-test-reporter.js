/**
 * Contract Test Reporter
 *
 * Enhanced reporter for Pact contract tests with metrics integration
 * Sends test results to metrics collector and generates comprehensive reports
 */

const axios = require('axios');
const fs = require('fs').promises;
const path = require('path');

class ContractTestReporter {
  constructor(options = {}) {
    this.metricsCollectorUrl =
      options.metricsCollectorUrl || 'http://localhost:9095';
    this.reportDir =
      options.reportDir ||
      path.resolve(process.cwd(), 'tests/contract/reports');
    this.enabled = options.enabled !== false;
    this.environment =
      options.environment || process.env.NODE_ENV || 'development';
    this.version = options.version || process.env.GIT_SHA || '1.0.0';

    // Ensure report directory exists
    this.ensureReportDir();
  }

  async ensureReportDir() {
    try {
      await fs.mkdir(this.reportDir, { recursive: true });
    } catch (error) {
      console.error('Failed to create report directory:', error);
    }
  }

  /**
   * Report contract test result to metrics collector
   */
  async reportTestResult(testResult) {
    if (!this.enabled) return;

    const payload = {
      ...testResult,
      environment: this.environment,
      version: this.version,
      timestamp: new Date().toISOString(),
    };

    try {
      // Send to metrics collector
      await axios.post(
        `${this.metricsCollectorUrl}/contract-test-result`,
        payload,
        {
          timeout: 5000,
        }
      );

      // Log structured result for log parsing
      console.log(`CONTRACT_TEST_RESULT: ${JSON.stringify(payload)}`);
    } catch (error) {
      console.warn(
        'Failed to report test result to metrics collector:',
        error.message
      );
      // Still log for backup parsing
      console.log(`CONTRACT_TEST_RESULT: ${JSON.stringify(payload)}`);
    }
  }

  /**
   * Report Pact publication to metrics collector
   */
  async reportPactPublication(publicationData) {
    if (!this.enabled) return;

    const payload = {
      ...publicationData,
      environment: this.environment,
      version: this.version,
      timestamp: new Date().toISOString(),
    };

    try {
      await axios.post(
        `${this.metricsCollectorUrl}/pact-publication`,
        payload,
        {
          timeout: 5000,
        }
      );

      console.log(`PACT_PUBLICATION: ${JSON.stringify(payload)}`);
    } catch (error) {
      console.warn(
        'Failed to report pact publication to metrics collector:',
        error.message
      );
      console.log(`PACT_PUBLICATION: ${JSON.stringify(payload)}`);
    }
  }

  /**
   * Generate comprehensive test report
   */
  async generateTestReport(results) {
    const timestamp = new Date().toISOString().replace(/[:.]/g, '-');
    const reportFile = path.join(
      this.reportDir,
      `contract-test-report-${timestamp}.json`
    );

    const report = {
      metadata: {
        environment: this.environment,
        version: this.version,
        timestamp: new Date().toISOString(),
        totalTests: results.length,
        passedTests: results.filter(r => r.status === 'passed').length,
        failedTests: results.filter(r => r.status === 'failed').length,
      },
      summary: this.generateSummary(results),
      results: results,
      coverage: await this.analyzeCoverage(results),
      trends: await this.analyzeTrends(),
    };

    try {
      await fs.writeFile(reportFile, JSON.stringify(report, null, 2));
      console.log(`Contract test report generated: ${reportFile}`);

      // Also generate HTML report for better visualization
      await this.generateHtmlReport(report, timestamp);

      return report;
    } catch (error) {
      console.error('Failed to generate test report:', error);
      throw error;
    }
  }

  generateSummary(results) {
    const summary = {
      byConsumerProvider: {},
      byTestType: {},
      errorTypes: {},
      performance: {
        averageDuration: 0,
        slowestTests: [],
      },
    };

    let totalDuration = 0;
    const testsWithDuration = results.filter(r => r.duration);

    results.forEach(result => {
      const key = `${result.consumer}->${result.provider}`;

      // By consumer/provider
      if (!summary.byConsumerProvider[key]) {
        summary.byConsumerProvider[key] = { passed: 0, failed: 0, total: 0 };
      }
      summary.byConsumerProvider[key].total++;
      summary.byConsumerProvider[key][
        result.status === 'passed' ? 'passed' : 'failed'
      ]++;

      // By test type
      const testType = result.testType || 'consumer';
      if (!summary.byTestType[testType]) {
        summary.byTestType[testType] = { passed: 0, failed: 0, total: 0 };
      }
      summary.byTestType[testType].total++;
      summary.byTestType[testType][
        result.status === 'passed' ? 'passed' : 'failed'
      ]++;

      // Error types
      if (result.status === 'failed' && result.errorType) {
        summary.errorTypes[result.errorType] =
          (summary.errorTypes[result.errorType] || 0) + 1;
      }

      // Performance
      if (result.duration) {
        totalDuration += result.duration;
        summary.performance.slowestTests.push({
          consumer: result.consumer,
          provider: result.provider,
          duration: result.duration,
        });
      }
    });

    // Calculate average duration
    if (testsWithDuration.length > 0) {
      summary.performance.averageDuration =
        totalDuration / testsWithDuration.length;
    }

    // Sort slowest tests
    summary.performance.slowestTests.sort((a, b) => b.duration - a.duration);
    summary.performance.slowestTests = summary.performance.slowestTests.slice(
      0,
      10
    );

    return summary;
  }

  async analyzeCoverage(results) {
    const coverage = {
      endpoints: new Map(),
      interactions: 0,
      totalEndpoints: 0,
      coveragePercentage: 0,
    };

    try {
      // Read pact files to analyze coverage
      const pactDir = path.resolve(process.cwd(), 'tests/contract/pacts');
      const pactFiles = await fs.readdir(pactDir).catch(() => []);

      for (const file of pactFiles) {
        if (file.endsWith('.json')) {
          const filePath = path.join(pactDir, file);
          const content = await fs.readFile(filePath, 'utf8');
          const pact = JSON.parse(content);

          if (pact.interactions) {
            coverage.interactions += pact.interactions.length;

            pact.interactions.forEach(interaction => {
              const method = interaction.request?.method || 'unknown';
              const path = interaction.request?.path || 'unknown';
              const key = `${method} ${path}`;

              coverage.endpoints.set(key, {
                method,
                path,
                consumer: pact.consumer?.name,
                provider: pact.provider?.name,
                covered: true,
              });
            });
          }
        }
      }

      coverage.totalEndpoints = coverage.endpoints.size;
      coverage.coveragePercentage =
        coverage.totalEndpoints > 0
          ? (coverage.interactions / coverage.totalEndpoints) * 100
          : 0;

      // Convert Map to object for JSON serialization
      coverage.endpoints = Object.fromEntries(coverage.endpoints);
    } catch (error) {
      console.error('Error analyzing coverage:', error);
    }

    return coverage;
  }

  async analyzeTrends() {
    try {
      // Read previous reports to analyze trends
      const reportFiles = await fs.readdir(this.reportDir).catch(() => []);
      const recentReports = [];

      // Get last 10 reports
      const reportPattern = /^contract-test-report-(.+)\.json$/;
      const sortedFiles = reportFiles
        .filter(file => reportPattern.test(file))
        .sort()
        .slice(-10);

      for (const file of sortedFiles) {
        try {
          const filePath = path.join(this.reportDir, file);
          const content = await fs.readFile(filePath, 'utf8');
          const report = JSON.parse(content);
          recentReports.push({
            timestamp: report.metadata.timestamp,
            totalTests: report.metadata.totalTests,
            passedTests: report.metadata.passedTests,
            failedTests: report.metadata.failedTests,
            successRate:
              report.metadata.totalTests > 0
                ? (report.metadata.passedTests / report.metadata.totalTests) *
                  100
                : 0,
          });
        } catch (error) {
          console.warn(`Failed to read report ${file}:`, error.message);
        }
      }

      return {
        recentRuns: recentReports,
        trend: this.calculateTrend(recentReports),
      };
    } catch (error) {
      console.error('Error analyzing trends:', error);
      return { recentRuns: [], trend: 'stable' };
    }
  }

  calculateTrend(reports) {
    if (reports.length < 2) return 'insufficient_data';

    const recent = reports.slice(-3);
    const older = reports.slice(-6, -3);

    if (recent.length === 0 || older.length === 0) return 'insufficient_data';

    const recentAvg =
      recent.reduce((sum, r) => sum + r.successRate, 0) / recent.length;
    const olderAvg =
      older.reduce((sum, r) => sum + r.successRate, 0) / older.length;

    const difference = recentAvg - olderAvg;

    if (Math.abs(difference) < 5) return 'stable';
    return difference > 0 ? 'improving' : 'declining';
  }

  async generateHtmlReport(report, timestamp) {
    const htmlFile = path.join(
      this.reportDir,
      `contract-test-report-${timestamp}.html`
    );

    const html = `
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Contract Test Report - ${report.metadata.timestamp}</title>
    <style>
        body { font-family: Arial, sans-serif; margin: 20px; background-color: #f5f5f5; }
        .container { max-width: 1200px; margin: 0 auto; background: white; padding: 20px; border-radius: 8px; }
        .header { text-align: center; border-bottom: 2px solid #007cba; padding-bottom: 20px; }
        .summary-cards { display: grid; grid-template-columns: repeat(auto-fit, minmax(250px, 1fr)); gap: 20px; margin: 20px 0; }
        .card { background: #f8f9fa; border-left: 4px solid #007cba; padding: 20px; border-radius: 4px; }
        .success { border-left-color: #28a745; }
        .failure { border-left-color: #dc3545; }
        .warning { border-left-color: #ffc107; }
        .metric { font-size: 2em; font-weight: bold; margin-bottom: 10px; }
        .table { width: 100%; border-collapse: collapse; margin: 20px 0; }
        .table th, .table td { border: 1px solid #dee2e6; padding: 12px; text-align: left; }
        .table th { background-color: #007cba; color: white; }
        .table tr:nth-child(even) { background-color: #f8f9fa; }
        .trend-improving { color: #28a745; }
        .trend-declining { color: #dc3545; }
        .trend-stable { color: #6c757d; }
    </style>
</head>
<body>
    <div class="container">
        <div class="header">
            <h1>WRFCoin Contract Test Report</h1>
            <p>Generated: ${report.metadata.timestamp}</p>
            <p>Environment: ${report.metadata.environment} | Version: ${report.metadata.version}</p>
        </div>

        <div class="summary-cards">
            <div class="card success">
                <div class="metric">${report.metadata.totalTests}</div>
                <div>Total Tests</div>
            </div>
            <div class="card ${report.metadata.passedTests === report.metadata.totalTests ? 'success' : 'warning'}">
                <div class="metric">${report.metadata.passedTests}</div>
                <div>Passed Tests</div>
            </div>
            <div class="card ${report.metadata.failedTests > 0 ? 'failure' : 'success'}">
                <div class="metric">${report.metadata.failedTests}</div>
                <div>Failed Tests</div>
            </div>
            <div class="card">
                <div class="metric">${((report.metadata.passedTests / report.metadata.totalTests) * 100).toFixed(1)}%</div>
                <div>Success Rate</div>
            </div>
        </div>

        <h2>Test Results by Consumer/Provider</h2>
        <table class="table">
            <thead>
                <tr>
                    <th>Consumer → Provider</th>
                    <th>Total</th>
                    <th>Passed</th>
                    <th>Failed</th>
                    <th>Success Rate</th>
                </tr>
            </thead>
            <tbody>
                ${Object.entries(report.summary.byConsumerProvider)
                  .map(
                    ([key, stats]) => `
                    <tr>
                        <td>${key}</td>
                        <td>${stats.total}</td>
                        <td>${stats.passed}</td>
                        <td>${stats.failed}</td>
                        <td>${((stats.passed / stats.total) * 100).toFixed(1)}%</td>
                    </tr>
                `
                  )
                  .join('')}
            </tbody>
        </table>

        <h2>Coverage Analysis</h2>
        <div class="summary-cards">
            <div class="card">
                <div class="metric">${report.coverage.totalEndpoints}</div>
                <div>Total Endpoints</div>
            </div>
            <div class="card">
                <div class="metric">${report.coverage.interactions}</div>
                <div>Total Interactions</div>
            </div>
            <div class="card">
                <div class="metric">${report.coverage.coveragePercentage.toFixed(1)}%</div>
                <div>Coverage Percentage</div>
            </div>
        </div>

        ${
          Object.keys(report.summary.errorTypes).length > 0
            ? `
        <h2>Error Analysis</h2>
        <table class="table">
            <thead>
                <tr>
                    <th>Error Type</th>
                    <th>Count</th>
                </tr>
            </thead>
            <tbody>
                ${Object.entries(report.summary.errorTypes)
                  .map(
                    ([error, count]) => `
                    <tr>
                        <td>${error}</td>
                        <td>${count}</td>
                    </tr>
                `
                  )
                  .join('')}
            </tbody>
        </table>
        `
            : ''
        }

        <h2>Performance Analysis</h2>
        <div class="card">
            <h3>Average Test Duration: ${report.summary.performance.averageDuration.toFixed(2)}s</h3>
            ${
              report.summary.performance.slowestTests.length > 0
                ? `
            <h4>Slowest Tests:</h4>
            <ul>
                ${report.summary.performance.slowestTests
                  .slice(0, 5)
                  .map(
                    test => `
                    <li>${test.consumer} → ${test.provider}: ${test.duration.toFixed(2)}s</li>
                `
                  )
                  .join('')}
            </ul>
            `
                : ''
            }
        </div>

        <h2>Trends</h2>
        <div class="card">
            <h3>Trend: <span class="trend-${report.trends.trend}">${report.trends.trend}</span></h3>
            ${
              report.trends.recentRuns.length > 0
                ? `
            <table class="table">
                <thead>
                    <tr>
                        <th>Run Time</th>
                        <th>Total Tests</th>
                        <th>Success Rate</th>
                    </tr>
                </thead>
                <tbody>
                    ${report.trends.recentRuns
                      .slice(-5)
                      .map(
                        run => `
                        <tr>
                            <td>${new Date(run.timestamp).toLocaleString()}</td>
                            <td>${run.totalTests}</td>
                            <td>${run.successRate.toFixed(1)}%</td>
                        </tr>
                    `
                      )
                      .join('')}
                </tbody>
            </table>
            `
                : ''
            }
        </div>

        <div class="footer" style="margin-top: 40px; text-align: center; color: #6c757d;">
            <p>Report generated by WRFCoin Contract Test Reporter</p>
        </div>
    </div>
</body>
</html>
    `;

    try {
      await fs.writeFile(htmlFile, html);
      console.log(`HTML contract test report generated: ${htmlFile}`);
    } catch (error) {
      console.error('Failed to generate HTML report:', error);
    }
  }

  /**
   * Middleware for Jest/test framework integration
   */
  createJestReporter() {
    return {
      onRunComplete: async (contexts, results) => {
        const testResults = [];

        results.testResults.forEach(testResult => {
          testResult.testResults.forEach(test => {
            // Extract consumer/provider from test name or file path
            const consumer = this.extractConsumerFromTest(test, testResult);
            const provider = this.extractProviderFromTest(test, testResult);

            testResults.push({
              consumer,
              provider,
              testType: 'consumer',
              status: test.status === 'passed' ? 'passed' : 'failed',
              duration: test.duration ? test.duration / 1000 : null,
              errorType:
                test.failureMessages?.length > 0 ? 'assertion_failed' : null,
              errorMessage: test.failureMessages?.join('; ') || null,
              testName: test.title,
            });

            // Report individual test result
            this.reportTestResult({
              consumer,
              provider,
              testType: 'consumer',
              status: test.status === 'passed' ? 'passed' : 'failed',
              duration: test.duration ? test.duration / 1000 : null,
              errorType:
                test.failureMessages?.length > 0 ? 'assertion_failed' : null,
              testName: test.title,
            });
          });
        });

        // Generate comprehensive report
        await this.generateTestReport(testResults);
      },
    };
  }

  extractConsumerFromTest(test, testResult) {
    // Try to extract from file path or test description
    const filePath = testResult.testFilePath || '';
    const testName = test.title || '';

    if (filePath.includes('api-postgres')) return 'wrfcoin-api-postgres';
    if (filePath.includes('api-blockchain')) return 'wrfcoin-api-blockchain';
    if (filePath.includes('api-weather')) return 'wrfcoin-api-weather';
    if (testName.includes('postgres')) return 'wrfcoin-api-postgres';
    if (testName.includes('blockchain')) return 'wrfcoin-api-blockchain';
    if (testName.includes('weather')) return 'wrfcoin-api-weather';

    return 'wrfcoin-api'; // Default
  }

  extractProviderFromTest(test, testResult) {
    // Try to extract from file path or test description
    const filePath = testResult.testFilePath || '';
    const testName = test.title || '';

    if (filePath.includes('postgres') || testName.includes('postgres'))
      return 'postgres-service';
    if (filePath.includes('blockchain') || testName.includes('blockchain'))
      return 'blockchain-service';
    if (filePath.includes('weather') || testName.includes('weather'))
      return 'weather-service';

    return 'unknown-provider';
  }
}

module.exports = ContractTestReporter;
