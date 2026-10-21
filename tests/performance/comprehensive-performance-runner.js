/**
 * Comprehensive Performance Test Runner
 * Orchestrates all performance tests and generates unified reports
 */

const fs = require('fs');
const path = require('path');
const { performance } = require('perf_hooks');
const { runAPILoadTests } = require('./api-load-tests');
const {
  runBlockchainPerformanceTests,
} = require('./blockchain-performance-tests');
const { UIPerformanceTester } = require('./ui-performance-tests');

class ComprehensivePerformanceRunner {
  constructor() {
    this.reportDir = path.join(__dirname, '../reports/performance');
    this.startTime = null;
    this.results = {
      api: null,
      blockchain: null,
      ui: null,
    };

    // Ensure report directory exists
    if (!fs.existsSync(this.reportDir)) {
      fs.mkdirSync(this.reportDir, { recursive: true });
    }
  }

  /**
   * Run all performance test suites
   */
  async runAllPerformanceTests(options = {}) {
    const {
      suites = ['api', 'blockchain', 'ui'],
      parallel = false,
      generateReport = true,
    } = options;

    console.log('🚀 Starting Comprehensive WRFCoin Performance Testing\n');
    console.log(`📋 Test Suites: ${suites.join(', ')}`);
    console.log(`⚡ Execution Mode: ${parallel ? 'Parallel' : 'Sequential'}\n`);

    this.startTime = performance.now();

    try {
      if (parallel) {
        await this.runTestsInParallel(suites);
      } else {
        await this.runTestsSequentially(suites);
      }

      if (generateReport) {
        return await this.generateUnifiedReport();
      }

      return this.results;
    } catch (error) {
      console.error('❌ Performance testing failed:', error);
      throw error;
    }
  }

  /**
   * Run test suites in parallel
   */
  async runTestsInParallel(suites) {
    const testPromises = [];

    if (suites.includes('api')) {
      testPromises.push(
        runAPILoadTests()
          .then(result => {
            this.results.api = result;
            console.log('✅ API Load Tests completed');
          })
          .catch(error => {
            console.error('❌ API Load Tests failed:', error.message);
            this.results.api = { error: error.message };
          })
      );
    }

    if (suites.includes('blockchain')) {
      testPromises.push(
        runBlockchainPerformanceTests()
          .then(result => {
            this.results.blockchain = result;
            console.log('✅ Blockchain Performance Tests completed');
          })
          .catch(error => {
            console.error(
              '❌ Blockchain Performance Tests failed:',
              error.message
            );
            this.results.blockchain = { error: error.message };
          })
      );
    }

    if (suites.includes('ui')) {
      testPromises.push(
        this.runUITests()
          .then(result => {
            this.results.ui = result;
            console.log('✅ UI Performance Tests completed');
          })
          .catch(error => {
            console.error('❌ UI Performance Tests failed:', error.message);
            this.results.ui = { error: error.message };
          })
      );
    }

    await Promise.all(testPromises);
  }

  /**
   * Run test suites sequentially
   */
  async runTestsSequentially(suites) {
    for (const suite of suites) {
      try {
        console.log(`\n🏃 Running ${suite.toUpperCase()} Performance Tests...`);

        switch (suite) {
          case 'api':
            this.results.api = await runAPILoadTests();
            break;
          case 'blockchain':
            this.results.blockchain = await runBlockchainPerformanceTests();
            break;
          case 'ui':
            this.results.ui = await this.runUITests();
            break;
          default:
            console.warn(`⚠️  Unknown test suite: ${suite}`);
        }

        console.log(`✅ ${suite.toUpperCase()} Performance Tests completed`);
      } catch (error) {
        console.error(
          `❌ ${suite.toUpperCase()} Performance Tests failed:`,
          error.message
        );
        this.results[suite] = { error: error.message };
      }
    }
  }

  /**
   * Run UI performance tests
   */
  async runUITests() {
    const uiTester = new UIPerformanceTester();

    // Dashboard performance
    await uiTester.runPerformanceTest({
      title: 'Dashboard Page Performance',
      url: '/dashboard',
      actions: [
        {
          type: 'click',
          selector: '[data-testid="weather-stations-tab"]',
          description: 'Switch to weather stations',
        },
        { type: 'scroll', value: 500, description: 'Scroll down page' },
      ],
      expectations: {
        maxFCP: 2000,
        maxLCP: 3000,
        maxCLS: 0.1,
        maxMemoryUsage: 50,
      },
    });

    // NFT Marketplace performance
    await uiTester.runPerformanceTest({
      title: 'NFT Marketplace Performance',
      url: '/nft-marketplace',
      actions: [
        { type: 'scroll', value: 1000, description: 'Scroll through NFT grid' },
      ],
      expectations: {
        maxFCP: 2500,
        maxLCP: 4000,
        maxCLS: 0.15,
        maxMemoryUsage: 75,
      },
    });

    // Wallet performance
    await uiTester.runPerformanceTest({
      title: 'Wallet Page Performance',
      url: '/wallet',
      actions: [
        {
          type: 'click',
          selector: '[data-testid="transaction-history-tab"]',
          description: 'Switch to transaction history',
        },
      ],
      expectations: {
        maxFCP: 1800,
        maxLCP: 2500,
        maxCLS: 0.05,
        maxMemoryUsage: 40,
      },
    });

    return uiTester.generateUIReport();
  }

  /**
   * Generate unified performance report
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
        testRunner: 'WRFCoin Comprehensive Performance Suite',
      },
      summary: {
        overallStatus: this.calculateOverallStatus(),
        performance: {
          api: this.summarizeAPIPerformance(),
          blockchain: this.summarizeBlockchainPerformance(),
          ui: this.summarizeUIPerformance(),
        },
        criticalIssues: this.identifyCriticalIssues(),
        recommendations: this.generateUnifiedRecommendations(),
      },
      detailed: this.results,
    };

    // Save unified report
    const reportPath = path.join(
      this.reportDir,
      'unified-performance-report.json'
    );
    fs.writeFileSync(reportPath, JSON.stringify(unifiedReport, null, 2));

    // Generate HTML dashboard
    await this.generateHTMLDashboard(unifiedReport);

    // Print summary
    this.printUnifiedSummary(unifiedReport);

    return unifiedReport;
  }

  /**
   * Calculate overall performance status
   */
  calculateOverallStatus() {
    const statuses = [];

    if (this.results.api && !this.results.api.error) {
      statuses.push(
        this.results.api.summary?.overallStatus === 'PASS' ? 'PASS' : 'FAIL'
      );
    }

    if (this.results.blockchain && !this.results.blockchain.error) {
      const networkHealth = this.results.blockchain.summary?.networkHealth;
      statuses.push(
        ['Excellent', 'Good'].includes(networkHealth) ? 'PASS' : 'FAIL'
      );
    }

    if (this.results.ui && !this.results.ui.error) {
      const performanceCategory = this.results.ui.summary?.performanceCategory;
      statuses.push(
        ['Excellent', 'Good'].includes(performanceCategory) ? 'PASS' : 'FAIL'
      );
    }

    const passCount = statuses.filter(s => s === 'PASS').length;
    const totalCount = statuses.length;

    if (totalCount === 0) return 'ERROR';
    if (passCount === totalCount) return 'EXCELLENT';
    if (passCount >= totalCount * 0.8) return 'GOOD';
    if (passCount >= totalCount * 0.6) return 'ACCEPTABLE';
    return 'POOR';
  }

  /**
   * Summarize API performance
   */
  summarizeAPIPerformance() {
    if (!this.results.api || this.results.api.error) {
      return { status: 'ERROR', message: this.results.api?.error || 'No data' };
    }

    const tests = this.results.api.tests || [];
    const avgRPS =
      tests.reduce((sum, test) => sum + (test.requests?.average || 0), 0) /
      tests.length;
    const avgLatency =
      tests.reduce((sum, test) => sum + (test.latency?.average || 0), 0) /
      tests.length;

    return {
      status: this.results.api.summary?.overallStatus || 'UNKNOWN',
      averageRPS: Math.round(avgRPS),
      averageLatency: Math.round(avgLatency),
      testsRun: tests.length,
      recommendations: this.results.api.recommendations?.length || 0,
    };
  }

  /**
   * Summarize blockchain performance
   */
  summarizeBlockchainPerformance() {
    if (!this.results.blockchain || this.results.blockchain.error) {
      return {
        status: 'ERROR',
        message: this.results.blockchain?.error || 'No data',
      };
    }

    return {
      status: this.results.blockchain.summary?.networkHealth || 'UNKNOWN',
      averageTPS: Math.round(this.results.blockchain.summary?.overallTPS || 0),
      testsRun: this.results.blockchain.tests?.length || 0,
      recommendations:
        this.results.blockchain.summary?.recommendations?.length || 0,
    };
  }

  /**
   * Summarize UI performance
   */
  summarizeUIPerformance() {
    if (!this.results.ui || this.results.ui.error) {
      return { status: 'ERROR', message: this.results.ui?.error || 'No data' };
    }

    return {
      status: this.results.ui.summary?.performanceCategory || 'UNKNOWN',
      averageLighthouseScore: Math.round(
        this.results.ui.summary?.averageLighthouseScore || 0
      ),
      testsRun: this.results.ui.tests?.length || 0,
      recommendations: this.results.ui.summary?.recommendations?.length || 0,
    };
  }

  /**
   * Identify critical performance issues
   */
  identifyCriticalIssues() {
    const issues = [];

    // API critical issues
    if (this.results.api?.tests) {
      this.results.api.tests.forEach(test => {
        if (test.requests?.average < 50) {
          issues.push({
            severity: 'CRITICAL',
            component: 'API',
            endpoint: test.url,
            issue: `Very low throughput: ${test.requests.average} RPS`,
            impact: 'High',
          });
        }
        if (test.latency?.p95 > 5000) {
          issues.push({
            severity: 'HIGH',
            component: 'API',
            endpoint: test.url,
            issue: `High P95 latency: ${test.latency.p95}ms`,
            impact: 'Medium',
          });
        }
      });
    }

    // Blockchain critical issues
    if (this.results.blockchain?.tests) {
      this.results.blockchain.tests.forEach(test => {
        if (test.performance?.tps < 5) {
          issues.push({
            severity: 'CRITICAL',
            component: 'Blockchain',
            operation: test.operation,
            issue: `Very low TPS: ${test.performance.tps}`,
            impact: 'High',
          });
        }
        if (test.performance?.successRate < 90) {
          issues.push({
            severity: 'HIGH',
            component: 'Blockchain',
            operation: test.operation,
            issue: `Low success rate: ${test.performance.successRate}%`,
            impact: 'High',
          });
        }
      });
    }

    // UI critical issues
    if (this.results.ui?.tests) {
      this.results.ui.tests.forEach(test => {
        if (test.lighthouse?.overallScore < 50) {
          issues.push({
            severity: 'HIGH',
            component: 'UI',
            page: test.url,
            issue: `Poor Lighthouse score: ${test.lighthouse.overallScore}/100`,
            impact: 'Medium',
          });
        }
        if (test.metrics?.initial?.timing?.largestContentfulPaint > 4000) {
          issues.push({
            severity: 'MEDIUM',
            component: 'UI',
            page: test.url,
            issue: `Slow LCP: ${test.metrics.initial.timing.largestContentfulPaint}ms`,
            impact: 'Medium',
          });
        }
      });
    }

    return issues;
  }

  /**
   * Generate unified recommendations
   */
  generateUnifiedRecommendations() {
    const recommendations = [];

    // Collect recommendations from all test suites
    if (this.results.api?.recommendations) {
      recommendations.push(
        ...this.results.api.recommendations.map(rec => ({
          ...rec,
          component: 'API',
          priority: this.assessRecommendationPriority(rec),
        }))
      );
    }

    if (this.results.blockchain?.summary?.recommendations) {
      recommendations.push(
        ...this.results.blockchain.summary.recommendations.map(rec => ({
          ...rec,
          component: 'Blockchain',
          priority: this.assessRecommendationPriority(rec),
        }))
      );
    }

    if (this.results.ui?.summary?.recommendations) {
      recommendations.push(
        ...this.results.ui.summary.recommendations.map(rec => ({
          ...rec,
          component: 'UI',
          priority: this.assessRecommendationPriority(rec),
        }))
      );
    }

    // Sort by priority
    return recommendations.sort((a, b) => {
      const priorityOrder = { High: 3, Medium: 2, Low: 1 };
      return priorityOrder[b.priority] - priorityOrder[a.priority];
    });
  }

  /**
   * Assess recommendation priority
   */
  assessRecommendationPriority(recommendation) {
    const highPriorityKeywords = [
      'critical',
      'slow',
      'high latency',
      'low throughput',
      'memory leak',
    ];
    const mediumPriorityKeywords = [
      'optimization',
      'improve',
      'reduce',
      'cache',
    ];

    const text = (
      recommendation.issue +
      ' ' +
      recommendation.recommendation
    ).toLowerCase();

    if (highPriorityKeywords.some(keyword => text.includes(keyword))) {
      return 'High';
    }
    if (mediumPriorityKeywords.some(keyword => text.includes(keyword))) {
      return 'Medium';
    }
    return 'Low';
  }

  /**
   * Generate HTML dashboard
   */
  async generateHTMLDashboard(reportData) {
    const htmlTemplate = `
<!DOCTYPE html>
<html>
<head>
    <title>WRFCoin Performance Dashboard</title>
    <style>
        body { font-family: Arial, sans-serif; margin: 0; padding: 20px; background: #f5f5f5; }
        .header { background: #2196F3; color: white; padding: 20px; border-radius: 8px; margin-bottom: 20px; }
        .summary-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(300px, 1fr)); gap: 20px; margin-bottom: 20px; }
        .summary-card { background: white; padding: 20px; border-radius: 8px; box-shadow: 0 2px 4px rgba(0,0,0,0.1); }
        .status-excellent { border-left: 5px solid #4CAF50; }
        .status-good { border-left: 5px solid #8BC34A; }
        .status-acceptable { border-left: 5px solid #FF9800; }
        .status-poor { border-left: 5px solid #f44336; }
        .status-error { border-left: 5px solid #9E9E9E; }
        .metric { display: flex; justify-content: space-between; margin: 10px 0; }
        .metric-value { font-weight: bold; }
        .issues-section { background: white; padding: 20px; border-radius: 8px; margin: 20px 0; }
        .issue-critical { background: #ffebee; border-left: 4px solid #f44336; padding: 10px; margin: 5px 0; }
        .issue-high { background: #fff3e0; border-left: 4px solid #ff9800; padding: 10px; margin: 5px 0; }
        .issue-medium { background: #f3e5f5; border-left: 4px solid #9c27b0; padding: 10px; margin: 5px 0; }
        .recommendations { background: white; padding: 20px; border-radius: 8px; margin: 20px 0; }
        .recommendation { background: #e8f5e8; border-left: 4px solid #4caf50; padding: 15px; margin: 10px 0; border-radius: 4px; }
        .timestamp { color: #666; font-size: 0.9em; }
    </style>
</head>
<body>
    <div class="header">
        <h1>🚀 WRFCoin Performance Dashboard</h1>
        <p class="timestamp">Generated: ${reportData.metadata.timestamp}</p>
        <p>Test Duration: ${reportData.metadata.duration}s | Overall Status: <strong>${reportData.summary.overallStatus}</strong></p>
    </div>

    <div class="summary-grid">
        <div class="summary-card status-${reportData.summary.performance.api.status.toLowerCase()}">
            <h3>📡 API Performance</h3>
            <div class="metric">
                <span>Status:</span>
                <span class="metric-value">${reportData.summary.performance.api.status}</span>
            </div>
            <div class="metric">
                <span>Average RPS:</span>
                <span class="metric-value">${reportData.summary.performance.api.averageRPS || 'N/A'}</span>
            </div>
            <div class="metric">
                <span>Average Latency:</span>
                <span class="metric-value">${reportData.summary.performance.api.averageLatency || 'N/A'}ms</span>
            </div>
            <div class="metric">
                <span>Tests Run:</span>
                <span class="metric-value">${reportData.summary.performance.api.testsRun}</span>
            </div>
        </div>

        <div class="summary-card status-${reportData.summary.performance.blockchain.status.toLowerCase()}">
            <h3>⛓️ Blockchain Performance</h3>
            <div class="metric">
                <span>Network Health:</span>
                <span class="metric-value">${reportData.summary.performance.blockchain.status}</span>
            </div>
            <div class="metric">
                <span>Average TPS:</span>
                <span class="metric-value">${reportData.summary.performance.blockchain.averageTPS || 'N/A'}</span>
            </div>
            <div class="metric">
                <span>Tests Run:</span>
                <span class="metric-value">${reportData.summary.performance.blockchain.testsRun}</span>
            </div>
        </div>

        <div class="summary-card status-${reportData.summary.performance.ui.status.toLowerCase()}">
            <h3>🎨 UI Performance</h3>
            <div class="metric">
                <span>Performance Category:</span>
                <span class="metric-value">${reportData.summary.performance.ui.status}</span>
            </div>
            <div class="metric">
                <span>Lighthouse Score:</span>
                <span class="metric-value">${reportData.summary.performance.ui.averageLighthouseScore || 'N/A'}/100</span>
            </div>
            <div class="metric">
                <span>Tests Run:</span>
                <span class="metric-value">${reportData.summary.performance.ui.testsRun}</span>
            </div>
        </div>
    </div>

    ${
      reportData.summary.criticalIssues.length > 0
        ? `
        <div class="issues-section">
            <h2>⚠️ Critical Issues (${reportData.summary.criticalIssues.length})</h2>
            ${reportData.summary.criticalIssues
              .map(
                issue => `
                <div class="issue-${issue.severity.toLowerCase()}">
                    <strong>${issue.component} - ${issue.severity}</strong><br>
                    ${issue.issue}<br>
                    <em>Impact: ${issue.impact}</em>
                </div>
            `
              )
              .join('')}
        </div>
    `
        : ''
    }

    <div class="recommendations">
        <h2>💡 Performance Recommendations (${reportData.summary.recommendations.length})</h2>
        ${reportData.summary.recommendations
          .slice(0, 10)
          .map(
            rec => `
            <div class="recommendation">
                <strong>${rec.component} - ${rec.priority} Priority</strong><br>
                <strong>Issue:</strong> ${rec.issue || rec.endpoint || rec.page}<br>
                <strong>Recommendation:</strong> ${rec.recommendation}
            </div>
        `
          )
          .join('')}
        ${reportData.summary.recommendations.length > 10 ? `<p><em>... and ${reportData.summary.recommendations.length - 10} more recommendations</em></p>` : ''}
    </div>
</body>
</html>`;

    const htmlPath = path.join(this.reportDir, 'performance-dashboard.html');
    fs.writeFileSync(htmlPath, htmlTemplate);

    console.log(`📊 Performance dashboard generated: ${htmlPath}`);
  }

  /**
   * Print unified summary
   */
  printUnifiedSummary(report) {
    console.log('\n' + '='.repeat(80));
    console.log('🚀 COMPREHENSIVE PERFORMANCE TEST SUMMARY');
    console.log('='.repeat(80));

    console.log(`\n📊 Overall Status: ${report.summary.overallStatus}`);
    console.log(`⏱️  Total Duration: ${report.metadata.duration}s`);

    console.log('\n📈 Component Performance:');
    console.log(
      `   📡 API: ${report.summary.performance.api.status} (${report.summary.performance.api.testsRun} tests)`
    );
    console.log(
      `   ⛓️  Blockchain: ${report.summary.performance.blockchain.status} (${report.summary.performance.blockchain.testsRun} tests)`
    );
    console.log(
      `   🎨 UI: ${report.summary.performance.ui.status} (${report.summary.performance.ui.testsRun} tests)`
    );

    if (report.summary.criticalIssues.length > 0) {
      console.log(
        `\n⚠️  Critical Issues: ${report.summary.criticalIssues.length}`
      );
      report.summary.criticalIssues.slice(0, 3).forEach(issue => {
        console.log(
          `   ${issue.severity}: ${issue.component} - ${issue.issue}`
        );
      });
      if (report.summary.criticalIssues.length > 3) {
        console.log(
          `   ... and ${report.summary.criticalIssues.length - 3} more issues`
        );
      }
    }

    if (report.summary.recommendations.length > 0) {
      console.log(`\n💡 Top Recommendations:`);
      report.summary.recommendations.slice(0, 3).forEach((rec, index) => {
        console.log(`   ${index + 1}. ${rec.component}: ${rec.recommendation}`);
      });
    }

    console.log(
      '\n📋 Detailed reports available in: test/reports/performance/'
    );
    console.log('='.repeat(80));
  }
}

// Main execution function
async function runComprehensivePerformanceTests(options = {}) {
  const runner = new ComprehensivePerformanceRunner();
  return await runner.runAllPerformanceTests(options);
}

// Export for use in other files
module.exports = {
  ComprehensivePerformanceRunner,
  runComprehensivePerformanceTests,
};

// Run tests if called directly
if (require.main === module) {
  const args = process.argv.slice(2);
  const options = {
    suites: args.includes('--api-only')
      ? ['api']
      : args.includes('--blockchain-only')
        ? ['blockchain']
        : args.includes('--ui-only')
          ? ['ui']
          : ['api', 'blockchain', 'ui'],
    parallel: args.includes('--parallel'),
    generateReport: !args.includes('--no-report'),
  };

  runComprehensivePerformanceTests(options)
    .then(report => {
      const status = report.summary?.overallStatus;
      process.exit(['EXCELLENT', 'GOOD'].includes(status) ? 0 : 1);
    })
    .catch(error => {
      console.error('Comprehensive performance testing failed:', error);
      process.exit(1);
    });
}
