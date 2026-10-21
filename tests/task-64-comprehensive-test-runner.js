/**
 * Task 64.6 - Comprehensive Test Runner
 *
 * Automated test suite runner for all mining control operations and real-time updates
 * Executes all test categories and generates comprehensive reports
 */

const { execSync, spawn } = require('child_process');
const fs = require('fs');
const path = require('path');

class Task64TestRunner {
  constructor() {
    this.results = {
      testSuite: 'Task 64.6 - Mining Control Operations & Real-Time Updates',
      timestamp: new Date().toISOString(),
      environment: this.getEnvironmentInfo(),
      testCategories: {},
      overallResults: {
        totalTests: 0,
        passed: 0,
        failed: 0,
        skipped: 0,
        duration: 0,
      },
      recommendations: [],
      status: 'PENDING',
    };
  }

  getEnvironmentInfo() {
    return {
      nodeVersion: process.version,
      platform: process.platform,
      arch: process.arch,
      environment: process.env.NODE_ENV || 'test',
      timestamp: new Date().toISOString(),
    };
  }

  async runTestCategory(category, testCommand, description) {
    console.log(`\n🧪 Running ${category}: ${description}`);
    console.log('=' * 60);

    const startTime = Date.now();
    let result;

    try {
      // Execute test command
      const output = execSync(testCommand, {
        encoding: 'utf8',
        cwd: process.cwd(),
        env: { ...process.env, NODE_ENV: 'test' },
      });

      const endTime = Date.now();
      const duration = endTime - startTime;

      result = {
        category,
        description,
        status: 'PASSED',
        duration: `${duration}ms`,
        output: output.substring(0, 1000), // Truncate long output
        testCount: this.extractTestCount(output),
        errors: [],
      };

      console.log(`✅ ${category} PASSED in ${duration}ms`);
    } catch (error) {
      const endTime = Date.now();
      const duration = endTime - startTime;

      result = {
        category,
        description,
        status: 'FAILED',
        duration: `${duration}ms`,
        output: error.stdout ? error.stdout.substring(0, 1000) : '',
        error: error.message,
        testCount: this.extractTestCount(error.stdout || ''),
        errors: [error.message],
      };

      console.log(`❌ ${category} FAILED in ${duration}ms`);
      console.log(`Error: ${error.message}`);
    }

    this.results.testCategories[category] = result;
    return result;
  }

  extractTestCount(output) {
    // Extract test counts from Jest/Playwright output
    const patterns = [
      /(\d+) passing/,
      /Tests:\s+(\d+) passed/,
      /(\d+) tests? passed/,
    ];

    for (const pattern of patterns) {
      const match = output.match(pattern);
      if (match) {
        return parseInt(match[1]);
      }
    }

    return 0;
  }

  async runAllTests() {
    console.log('🚀 Starting Task 64.6 Comprehensive Test Suite');
    console.log(`Timestamp: ${this.results.timestamp}`);
    console.log(
      `Environment: ${JSON.stringify(this.results.environment, null, 2)}`
    );

    const overallStartTime = Date.now();

    // Test Categories with their respective commands
    const testCategories = [
      {
        category: 'integration',
        command:
          'npm test -- tests/integration/mining-control-operations.test.js',
        description: 'Mining Control Operations Integration Tests',
      },
      {
        category: 'frontend',
        command:
          'npm test -- tests/frontend/mining-dashboard-components.test.jsx',
        description: 'Frontend React Components Tests',
      },
      {
        category: 'performance',
        command:
          'npm test -- tests/performance/mining-operations-performance.test.js',
        description: 'Performance and Load Testing',
      },
      {
        category: 'e2e',
        command: 'npx playwright test tests/e2e/mining-dashboard-e2e.test.js',
        description: 'End-to-End User Workflow Tests',
      },
      {
        category: 'api',
        command: 'npm test -- --testPathPattern="api.*mining"',
        description: 'Backend API Endpoint Tests',
      },
      {
        category: 'websocket',
        command: 'npm test -- --testPathPattern="websocket.*mining"',
        description: 'WebSocket Real-Time Communication Tests',
      },
    ];

    // Run each test category
    for (const testCategory of testCategories) {
      try {
        await this.runTestCategory(
          testCategory.category,
          testCategory.command,
          testCategory.description
        );
      } catch (error) {
        console.error(`Failed to run ${testCategory.category}:`, error);
      }
    }

    const overallEndTime = Date.now();
    this.results.overallResults.duration = overallEndTime - overallStartTime;

    // Calculate overall statistics
    this.calculateOverallResults();
    this.generateRecommendations();
    this.determineOverallStatus();

    // Generate reports
    await this.generateReports();

    return this.results;
  }

  calculateOverallResults() {
    let totalTests = 0;
    let passed = 0;
    let failed = 0;

    Object.values(this.results.testCategories).forEach(category => {
      totalTests += category.testCount;
      if (category.status === 'PASSED') {
        passed += category.testCount;
      } else {
        failed += category.testCount;
      }
    });

    this.results.overallResults.totalTests = totalTests;
    this.results.overallResults.passed = passed;
    this.results.overallResults.failed = failed;
  }

  generateRecommendations() {
    const recommendations = [];

    // Analyze test results and generate recommendations
    Object.entries(this.results.testCategories).forEach(
      ([category, result]) => {
        if (result.status === 'PASSED') {
          recommendations.push(
            `✅ ${category}: All tests passed - feature is production ready`
          );
        } else {
          recommendations.push(
            `⚠️ ${category}: Tests failed - requires attention before deployment`
          );
          if (result.errors.length > 0) {
            recommendations.push(`   - Error: ${result.errors[0]}`);
          }
        }
      }
    );

    // Add general recommendations
    if (this.results.overallResults.failed === 0) {
      recommendations.push(
        '🎉 All test categories passed - Task 64 implementation is complete and production ready'
      );
      recommendations.push(
        '📊 Real-time mining statistics display is functioning correctly'
      );
      recommendations.push(
        '🎛️ Mining control operations (start/pause/resume/restart/stop) are working properly'
      );
      recommendations.push(
        '📈 Performance monitoring dashboard is operational'
      );
      recommendations.push(
        '⚙️ Configuration management interface is fully functional'
      );
      recommendations.push(
        '🔗 Integration with existing one-click mining is seamless'
      );
    } else {
      recommendations.push(
        `❌ ${this.results.overallResults.failed} tests failed - review and fix before deployment`
      );
      recommendations.push(
        '🔍 Check individual test category results for specific issues'
      );
    }

    this.results.recommendations = recommendations;
  }

  determineOverallStatus() {
    const failedCategories = Object.values(this.results.testCategories).filter(
      cat => cat.status === 'FAILED'
    ).length;

    if (failedCategories === 0) {
      this.results.status = 'PRODUCTION READY';
    } else if (failedCategories <= 2) {
      this.results.status = 'NEEDS MINOR FIXES';
    } else {
      this.results.status = 'NEEDS MAJOR FIXES';
    }
  }

  async generateReports() {
    // Generate JSON report
    const jsonReportPath = path.join(
      __dirname,
      'reports',
      'task-64-test-results.json'
    );
    await this.ensureDirectoryExists(path.dirname(jsonReportPath));
    fs.writeFileSync(jsonReportPath, JSON.stringify(this.results, null, 2));

    // Generate HTML report
    const htmlReport = this.generateHTMLReport();
    const htmlReportPath = path.join(
      __dirname,
      'reports',
      'task-64-test-results.html'
    );
    fs.writeFileSync(htmlReportPath, htmlReport);

    // Generate markdown summary
    const markdownReport = this.generateMarkdownReport();
    const markdownReportPath = path.join(
      __dirname,
      'reports',
      'task-64-test-summary.md'
    );
    fs.writeFileSync(markdownReportPath, markdownReport);

    console.log('\n📊 Reports Generated:');
    console.log(`   - JSON: ${jsonReportPath}`);
    console.log(`   - HTML: ${htmlReportPath}`);
    console.log(`   - Markdown: ${markdownReportPath}`);
  }

  generateHTMLReport() {
    const statusColor =
      this.results.status === 'PRODUCTION READY'
        ? '#4CAF50'
        : this.results.status === 'NEEDS MINOR FIXES'
          ? '#FF9800'
          : '#F44336';

    return `
<!DOCTYPE html>
<html>
<head>
    <title>Task 64.6 - Mining Control Operations Test Results</title>
    <style>
        body { font-family: Arial, sans-serif; margin: 20px; background: #f5f5f5; }
        .container { max-width: 1200px; margin: 0 auto; background: white; padding: 20px; border-radius: 8px; box-shadow: 0 2px 10px rgba(0,0,0,0.1); }
        .header { text-align: center; border-bottom: 2px solid #eee; padding-bottom: 20px; margin-bottom: 20px; }
        .status { display: inline-block; padding: 10px 20px; border-radius: 20px; color: white; font-weight: bold; background: ${statusColor}; }
        .summary { display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 20px; margin: 20px 0; }
        .metric { background: #f8f9fa; padding: 15px; border-radius: 8px; text-align: center; }
        .metric-value { font-size: 2em; font-weight: bold; color: #333; }
        .metric-label { color: #666; margin-top: 5px; }
        .category { margin: 20px 0; border: 1px solid #ddd; border-radius: 8px; overflow: hidden; }
        .category-header { background: #007bff; color: white; padding: 15px; font-weight: bold; }
        .category-content { padding: 15px; }
        .passed { border-left: 4px solid #4CAF50; }
        .failed { border-left: 4px solid #F44336; }
        .recommendations { background: #e3f2fd; padding: 20px; border-radius: 8px; margin: 20px 0; }
        .recommendation { margin: 10px 0; padding: 10px; background: white; border-radius: 4px; }
    </style>
</head>
<body>
    <div class="container">
        <div class="header">
            <h1>Task 64.6 - Mining Control Operations Test Results</h1>
            <div class="status">${this.results.status}</div>
            <p>Generated: ${this.results.timestamp}</p>
        </div>

        <div class="summary">
            <div class="metric">
                <div class="metric-value">${this.results.overallResults.totalTests}</div>
                <div class="metric-label">Total Tests</div>
            </div>
            <div class="metric">
                <div class="metric-value" style="color: #4CAF50">${this.results.overallResults.passed}</div>
                <div class="metric-label">Passed</div>
            </div>
            <div class="metric">
                <div class="metric-value" style="color: #F44336">${this.results.overallResults.failed}</div>
                <div class="metric-label">Failed</div>
            </div>
            <div class="metric">
                <div class="metric-value">${this.results.overallResults.duration}ms</div>
                <div class="metric-label">Duration</div>
            </div>
        </div>

        <h2>Test Categories</h2>
        ${Object.entries(this.results.testCategories)
          .map(
            ([category, result]) => `
            <div class="category ${result.status.toLowerCase()}">
                <div class="category-header">
                    ${result.status === 'PASSED' ? '✅' : '❌'} ${category.toUpperCase()}: ${result.description}
                </div>
                <div class="category-content">
                    <p><strong>Status:</strong> ${result.status}</p>
                    <p><strong>Duration:</strong> ${result.duration}</p>
                    <p><strong>Tests:</strong> ${result.testCount}</p>
                    ${result.error ? `<p><strong>Error:</strong> ${result.error}</p>` : ''}
                </div>
            </div>
        `
          )
          .join('')}

        <div class="recommendations">
            <h2>Recommendations</h2>
            ${this.results.recommendations
              .map(
                rec => `
                <div class="recommendation">${rec}</div>
            `
              )
              .join('')}
        </div>
    </div>
</body>
</html>`;
  }

  generateMarkdownReport() {
    return `# Task 64.6 - Mining Control Operations Test Results

## Overview
- **Status:** ${this.results.status}
- **Generated:** ${this.results.timestamp}
- **Total Tests:** ${this.results.overallResults.totalTests}
- **Passed:** ${this.results.overallResults.passed}
- **Failed:** ${this.results.overallResults.failed}
- **Duration:** ${this.results.overallResults.duration}ms

## Test Categories

${Object.entries(this.results.testCategories)
  .map(
    ([category, result]) => `
### ${category.toUpperCase()}: ${result.description}
- **Status:** ${result.status === 'PASSED' ? '✅ PASSED' : '❌ FAILED'}
- **Duration:** ${result.duration}
- **Tests:** ${result.testCount}
${result.error ? `- **Error:** ${result.error}` : ''}
`
  )
  .join('')}

## Recommendations

${this.results.recommendations.map(rec => `- ${rec}`).join('\n')}

## Environment

\`\`\`json
${JSON.stringify(this.results.environment, null, 2)}
\`\`\`

---
*Generated by Task 64.6 Comprehensive Test Runner*`;
  }

  async ensureDirectoryExists(dir) {
    if (!fs.existsSync(dir)) {
      fs.mkdirSync(dir, { recursive: true });
    }
  }

  displayFinalResults() {
    console.log('\n' + '='.repeat(80));
    console.log('🎯 TASK 64.6 - COMPREHENSIVE TEST RESULTS');
    console.log('='.repeat(80));
    console.log(`Status: ${this.results.status}`);
    console.log(`Total Tests: ${this.results.overallResults.totalTests}`);
    console.log(`Passed: ${this.results.overallResults.passed}`);
    console.log(`Failed: ${this.results.overallResults.failed}`);
    console.log(`Duration: ${this.results.overallResults.duration}ms`);
    console.log('\nTest Categories:');

    Object.entries(this.results.testCategories).forEach(
      ([category, result]) => {
        const icon = result.status === 'PASSED' ? '✅' : '❌';
        console.log(
          `  ${icon} ${category}: ${result.status} (${result.testCount} tests, ${result.duration})`
        );
      }
    );

    console.log('\n📋 Key Recommendations:');
    this.results.recommendations.slice(0, 5).forEach(rec => {
      console.log(`  ${rec}`);
    });

    console.log('\n' + '='.repeat(80));

    if (this.results.status === 'PRODUCTION READY') {
      console.log('🎉 ALL TESTS PASSED - TASK 64 IS PRODUCTION READY! 🎉');
    } else {
      console.log('⚠️  SOME TESTS FAILED - REVIEW REQUIRED BEFORE DEPLOYMENT');
    }

    console.log('='.repeat(80));
  }
}

// CLI Interface
if (require.main === module) {
  const runner = new Task64TestRunner();

  async function main() {
    try {
      console.log('Starting Task 64.6 Comprehensive Testing...\n');

      const results = await runner.runAllTests();
      runner.displayFinalResults();

      // Exit with appropriate code
      process.exit(results.overallResults.failed > 0 ? 1 : 0);
    } catch (error) {
      console.error('❌ Test runner failed:', error);
      process.exit(1);
    }
  }

  main();
}

module.exports = Task64TestRunner;
