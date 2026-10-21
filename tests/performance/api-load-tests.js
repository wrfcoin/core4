/**
 * API Load Testing Suite
 * Tests API endpoints under various load conditions using autocannon
 */

const autocannon = require('autocannon');
const { performance } = require('perf_hooks');
const fs = require('fs');
const path = require('path');
const config = require('../test.config');

class APILoadTester {
  constructor() {
    this.baseUrl = config.baseUrls.backend;
    this.results = [];
    this.reportDir = path.join(__dirname, '../reports/performance');

    // Ensure report directory exists
    if (!fs.existsSync(this.reportDir)) {
      fs.mkdirSync(this.reportDir, { recursive: true });
    }
  }

  /**
   * Execute load test with specified parameters
   */
  async executeLoadTest(testConfig) {
    const {
      url,
      method = 'GET',
      headers = { 'Content-Type': 'application/json' },
      body,
      connections = 10,
      pipelining = 1,
      duration = 10,
      amount,
      rate,
      title = 'Load Test',
      expectation = {},
    } = testConfig;

    console.log(`🚀 Starting ${title}...`);
    console.log(`   URL: ${this.baseUrl}${url}`);
    console.log(`   Connections: ${connections}, Duration: ${duration}s`);

    const startTime = performance.now();

    const result = await autocannon({
      url: `${this.baseUrl}${url}`,
      method,
      headers,
      body: body ? JSON.stringify(body) : undefined,
      connections,
      pipelining,
      duration,
      amount,
      rate,
      timeout: 30000,
      setupClient: client => {
        // Add authentication headers if needed
        if (headers.Authorization) {
          client.setHeaders(headers);
        }
      },
    });

    const endTime = performance.now();
    const testDuration = (endTime - startTime) / 1000;

    // Process results
    const processedResult = {
      title,
      url,
      method,
      connections,
      duration: testDuration,
      requests: {
        total: result.requests.total,
        average: Math.round(result.requests.average),
        mean: Math.round(result.requests.mean),
        stddev: Math.round(result.requests.stddev),
        min: result.requests.min,
        max: result.requests.max,
        p50: result.requests.p50,
        p90: result.requests.p90,
        p95: result.requests.p95,
        p99: result.requests.p99,
      },
      latency: {
        average: result.latency.average,
        mean: result.latency.mean,
        stddev: result.latency.stddev,
        min: result.latency.min,
        max: result.latency.max,
        p50: result.latency.p50,
        p90: result.latency.p90,
        p95: result.latency.p95,
        p99: result.latency.p99,
      },
      throughput: {
        average: Math.round(result.throughput.average),
        mean: Math.round(result.throughput.mean),
        stddev: Math.round(result.throughput.stddev),
        min: result.throughput.min,
        max: result.throughput.max,
      },
      errors: result.errors,
      timeouts: result.timeouts,
      non2xx: result.non2xx || 0,
      timestamp: new Date().toISOString(),
      passed: this.evaluatePerformance(result, expectation),
    };

    this.results.push(processedResult);
    this.printResults(processedResult);

    return processedResult;
  }

  /**
   * Evaluate if performance meets expectations
   */
  evaluatePerformance(result, expectation) {
    const checks = [];

    if (expectation.minRPS) {
      checks.push({
        check: 'Minimum RPS',
        expected: expectation.minRPS,
        actual: result.requests.average,
        passed: result.requests.average >= expectation.minRPS,
      });
    }

    if (expectation.maxLatencyP95) {
      checks.push({
        check: 'Max P95 Latency',
        expected: `${expectation.maxLatencyP95}ms`,
        actual: `${result.latency.p95}ms`,
        passed: result.latency.p95 <= expectation.maxLatencyP95,
      });
    }

    if (expectation.maxErrorRate) {
      const errorRate = (result.errors / result.requests.total) * 100;
      checks.push({
        check: 'Max Error Rate',
        expected: `${expectation.maxErrorRate}%`,
        actual: `${errorRate.toFixed(2)}%`,
        passed: errorRate <= expectation.maxErrorRate,
      });
    }

    return checks;
  }

  /**
   * Print formatted test results
   */
  printResults(result) {
    console.log(`\n📊 ${result.title} Results:`);
    console.log(
      `   Requests/sec: ${result.requests.average} (avg), ${result.requests.max} (max)`
    );
    console.log(
      `   Latency P95: ${result.latency.p95}ms, P99: ${result.latency.p99}ms`
    );
    console.log(`   Throughput: ${result.throughput.average} bytes/sec`);
    console.log(`   Errors: ${result.errors}, Timeouts: ${result.timeouts}`);

    if (result.passed && result.passed.length > 0) {
      console.log(`   Performance Checks:`);
      result.passed.forEach(check => {
        const status = check.passed ? '✅' : '❌';
        console.log(
          `     ${status} ${check.check}: ${check.actual} (expected: ${check.expected})`
        );
      });
    }
  }

  /**
   * Generate comprehensive performance report
   */
  generateReport() {
    const reportData = {
      summary: {
        totalTests: this.results.length,
        timestamp: new Date().toISOString(),
        overallStatus: this.results.every(
          r => r.passed?.every(p => p.passed) ?? true
        )
          ? 'PASS'
          : 'FAIL',
      },
      tests: this.results,
      recommendations: this.generateRecommendations(),
    };

    // Save JSON report
    const jsonReport = path.join(this.reportDir, 'api-load-test-results.json');
    fs.writeFileSync(jsonReport, JSON.stringify(reportData, null, 2));

    // Generate HTML report
    this.generateHTMLReport(reportData);

    console.log(`\n📋 Performance report generated:`);
    console.log(`   JSON: ${jsonReport}`);
    console.log(
      `   HTML: ${path.join(this.reportDir, 'api-load-test-report.html')}`
    );

    return reportData;
  }

  /**
   * Generate performance improvement recommendations
   */
  generateRecommendations() {
    const recommendations = [];

    this.results.forEach(result => {
      // High latency detection
      if (result.latency.p95 > 1000) {
        recommendations.push({
          endpoint: result.url,
          issue: 'High P95 Latency',
          value: `${result.latency.p95}ms`,
          recommendation:
            'Consider database query optimization, caching, or connection pooling',
        });
      }

      // Low throughput detection
      if (result.requests.average < 100) {
        recommendations.push({
          endpoint: result.url,
          issue: 'Low Request Throughput',
          value: `${result.requests.average} RPS`,
          recommendation:
            'Review server configuration, add horizontal scaling, or optimize business logic',
        });
      }

      // Error rate detection
      if (result.errors > 0) {
        recommendations.push({
          endpoint: result.url,
          issue: 'Request Errors',
          value: `${result.errors} errors`,
          recommendation:
            'Investigate error logs, add proper error handling, and review server capacity',
        });
      }
    });

    return recommendations;
  }

  /**
   * Generate HTML report
   */
  generateHTMLReport(data) {
    const htmlTemplate = `
<!DOCTYPE html>
<html>
<head>
    <title>WRFCoin API Load Test Report</title>
    <style>
        body { font-family: Arial, sans-serif; margin: 20px; }
        .summary { background: #f5f5f5; padding: 15px; border-radius: 5px; margin-bottom: 20px; }
        .test-result { border: 1px solid #ddd; margin: 10px 0; padding: 15px; border-radius: 5px; }
        .pass { border-left: 5px solid #4CAF50; }
        .fail { border-left: 5px solid #f44336; }
        .metrics { display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 10px; }
        .metric { background: #f9f9f9; padding: 10px; border-radius: 3px; }
        .recommendations { background: #fff3cd; padding: 15px; border-radius: 5px; margin-top: 20px; }
        table { width: 100%; border-collapse: collapse; margin: 10px 0; }
        th, td { border: 1px solid #ddd; padding: 8px; text-align: left; }
        th { background-color: #f2f2f2; }
    </style>
</head>
<body>
    <h1>WRFCoin API Load Test Report</h1>
    
    <div class="summary">
        <h2>Test Summary</h2>
        <p><strong>Total Tests:</strong> ${data.summary.totalTests}</p>
        <p><strong>Generated:</strong> ${data.summary.timestamp}</p>
        <p><strong>Overall Status:</strong> <span style="color: ${data.summary.overallStatus === 'PASS' ? 'green' : 'red'}">${data.summary.overallStatus}</span></p>
    </div>

    <h2>Test Results</h2>
    ${data.tests
      .map(
        test => `
        <div class="test-result ${test.passed?.every(p => p.passed) ? 'pass' : 'fail'}">
            <h3>${test.title}</h3>
            <p><strong>Endpoint:</strong> ${test.method} ${test.url}</p>
            <div class="metrics">
                <div class="metric">
                    <strong>Requests/sec</strong><br>
                    Avg: ${test.requests.average}<br>
                    P95: ${test.requests.p95}
                </div>
                <div class="metric">
                    <strong>Latency (ms)</strong><br>
                    P95: ${test.latency.p95}<br>
                    P99: ${test.latency.p99}
                </div>
                <div class="metric">
                    <strong>Throughput</strong><br>
                    ${test.throughput.average} bytes/sec
                </div>
                <div class="metric">
                    <strong>Errors</strong><br>
                    ${test.errors} errors<br>
                    ${test.timeouts} timeouts
                </div>
            </div>
            ${
              test.passed && test.passed.length > 0
                ? `
                <h4>Performance Checks</h4>
                <table>
                    <tr><th>Check</th><th>Expected</th><th>Actual</th><th>Status</th></tr>
                    ${test.passed
                      .map(
                        check => `
                        <tr>
                            <td>${check.check}</td>
                            <td>${check.expected}</td>
                            <td>${check.actual}</td>
                            <td style="color: ${check.passed ? 'green' : 'red'}">${check.passed ? 'PASS' : 'FAIL'}</td>
                        </tr>
                    `
                      )
                      .join('')}
                </table>
            `
                : ''
            }
        </div>
    `
      )
      .join('')}

    ${
      data.recommendations.length > 0
        ? `
        <div class="recommendations">
            <h2>Performance Recommendations</h2>
            <table>
                <tr><th>Endpoint</th><th>Issue</th><th>Current Value</th><th>Recommendation</th></tr>
                ${data.recommendations
                  .map(
                    rec => `
                    <tr>
                        <td>${rec.endpoint}</td>
                        <td>${rec.issue}</td>
                        <td>${rec.value}</td>
                        <td>${rec.recommendation}</td>
                    </tr>
                `
                  )
                  .join('')}
            </table>
        </div>
    `
        : ''
    }
</body>
</html>`;

    const htmlReport = path.join(this.reportDir, 'api-load-test-report.html');
    fs.writeFileSync(htmlReport, htmlTemplate);
  }
}

// Main test execution
async function runAPILoadTests() {
  const tester = new APILoadTester();

  console.log('🔥 Starting WRFCoin API Load Testing Suite\n');

  try {
    // Test 1: Authentication endpoint
    await tester.executeLoadTest({
      title: 'Authentication Login',
      url: '/api/auth/login',
      method: 'POST',
      body: {
        email: 'test@wrfcoin.test',
        password: 'testpass123',
      },
      connections: 50,
      duration: 30,
      expectation: {
        minRPS: 100,
        maxLatencyP95: 500,
        maxErrorRate: 1,
      },
    });

    // Test 2: Wallet balance endpoint
    await tester.executeLoadTest({
      title: 'Wallet Balance Retrieval',
      url: '/api/wallet/balance',
      method: 'GET',
      headers: {
        Authorization: 'Bearer test-token',
        'Content-Type': 'application/json',
      },
      connections: 100,
      duration: 30,
      expectation: {
        minRPS: 200,
        maxLatencyP95: 300,
        maxErrorRate: 0.5,
      },
    });

    // Test 3: NFT marketplace listing
    await tester.executeLoadTest({
      title: 'NFT Marketplace Listings',
      url: '/api/nft/marketplace',
      method: 'GET',
      connections: 75,
      duration: 30,
      expectation: {
        minRPS: 150,
        maxLatencyP95: 800,
        maxErrorRate: 1,
      },
    });

    // Test 4: Weather data submission
    await tester.executeLoadTest({
      title: 'Weather Data Submission',
      url: '/api/weather/submit',
      method: 'POST',
      body: {
        stationId: 'test-station-1',
        temperature: 72.5,
        humidity: 65,
        pressure: 1013.25,
        timestamp: new Date().toISOString(),
      },
      connections: 25,
      duration: 30,
      expectation: {
        minRPS: 50,
        maxLatencyP95: 1000,
        maxErrorRate: 2,
      },
    });

    // Test 5: Governance proposals
    await tester.executeLoadTest({
      title: 'Governance Proposals List',
      url: '/api/governance/proposals',
      method: 'GET',
      connections: 40,
      duration: 20,
      expectation: {
        minRPS: 80,
        maxLatencyP95: 600,
        maxErrorRate: 1,
      },
    });

    // Test 6: Cross-chain bridge status
    await tester.executeLoadTest({
      title: 'Cross-Chain Bridge Status',
      url: '/api/bridge/status',
      method: 'GET',
      connections: 30,
      duration: 20,
      expectation: {
        minRPS: 100,
        maxLatencyP95: 400,
        maxErrorRate: 0.5,
      },
    });

    // Test 7: Dashboard data aggregation (high load)
    await tester.executeLoadTest({
      title: 'Dashboard Data Aggregation',
      url: '/api/dashboard/summary',
      method: 'GET',
      headers: {
        Authorization: 'Bearer test-token',
        'Content-Type': 'application/json',
      },
      connections: 200,
      duration: 45,
      expectation: {
        minRPS: 100,
        maxLatencyP95: 1500,
        maxErrorRate: 2,
      },
    });

    // Test 8: Stress test - Transaction processing
    await tester.executeLoadTest({
      title: 'Transaction Processing Stress Test',
      url: '/api/transactions/process',
      method: 'POST',
      body: {
        from: 'wrf717d72395303fcb2aed4df368f32f1078d18e9cc',
        to: 'wrf827e83406414fcb3bfe5ef479f43f2189e29fdd',
        amount: '10.5',
        type: 'transfer',
      },
      connections: 300,
      duration: 60,
      expectation: {
        minRPS: 50,
        maxLatencyP95: 2000,
        maxErrorRate: 5,
      },
    });

    // Generate comprehensive report
    const report = tester.generateReport();

    console.log('\n🎯 Load Testing Complete!');
    console.log(`📊 Overall Status: ${report.summary.overallStatus}`);

    if (report.recommendations.length > 0) {
      console.log(
        `⚠️  Found ${report.recommendations.length} performance recommendations`
      );
    }

    return report;
  } catch (error) {
    console.error('❌ Load testing failed:', error);
    throw error;
  }
}

// Export for use in other test files
module.exports = {
  APILoadTester,
  runAPILoadTests,
};

// Run tests if called directly
if (require.main === module) {
  runAPILoadTests()
    .then(report => {
      process.exit(report.summary.overallStatus === 'PASS' ? 0 : 1);
    })
    .catch(error => {
      console.error('Test execution failed:', error);
      process.exit(1);
    });
}
