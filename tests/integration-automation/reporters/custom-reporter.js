/**
 * Custom Integration Test Automation Reporter
 * Provides detailed metrics and insights for integration test execution
 */

const fs = require('fs');
const path = require('path');

class IntegrationAutomationReporter {
  constructor(globalConfig, options = {}) {
    this.globalConfig = globalConfig;
    this.options = {
      outputFile:
        'tests/results/integration-automation/automation-metrics.json',
      generateTrends: true,
      trackPerformance: true,
      alertThresholds: {
        successRate: 95, // Alert if success rate below 95%
        avgDuration: 60000, // Alert if average test duration above 60s
        failureRate: 5, // Alert if failure rate above 5%
      },
      ...options,
    };

    this.metrics = {
      startTime: null,
      endTime: null,
      totalDuration: 0,
      suiteMetrics: {},
      testMetrics: {},
      performanceData: [],
      trendsData: [],
      alerts: [],
      summary: {},
    };

    this.testState = {
      currentSuite: null,
      suiteStartTime: null,
      testsCompleted: 0,
      testsTotal: 0,
      consecutiveFailures: 0,
    };
  }

  /**
   * Called when test run starts
   */
  onRunStart(aggregatedResult, options) {
    this.metrics.startTime = new Date();
    this.testState.testsTotal = aggregatedResult.numTotalTestSuites;

    console.log('\n🚀 Integration Test Automation Started');
    console.log(`📊 Suites to execute: ${this.testState.testsTotal}`);
    console.log(
      `🎯 Target success rate: ${this.options.alertThresholds.successRate}%`
    );
    console.log(
      `⏱️  Duration threshold: ${this.options.alertThresholds.avgDuration / 1000}s`
    );

    // Initialize results directory
    const resultsDir = path.dirname(this.options.outputFile);
    if (!fs.existsSync(resultsDir)) {
      fs.mkdirSync(resultsDir, { recursive: true });
    }
  }

  /**
   * Called when test suite starts
   */
  onTestSuiteStart(test) {
    this.testState.currentSuite = test.path;
    this.testState.suiteStartTime = Date.now();

    const suiteName = this.getSuiteName(test.path);
    console.log(`\n🏃 Starting: ${suiteName}`);

    // Initialize suite metrics
    this.metrics.suiteMetrics[test.path] = {
      name: suiteName,
      startTime: this.testState.suiteStartTime,
      endTime: null,
      duration: 0,
      tests: {
        total: 0,
        passed: 0,
        failed: 0,
        skipped: 0,
        todo: 0,
      },
      performance: {
        avgTestDuration: 0,
        slowestTest: null,
        fastestTest: null,
        memoryUsage: null,
      },
      coverage: {
        statements: 0,
        branches: 0,
        functions: 0,
        lines: 0,
      },
      errors: [],
      warnings: [],
    };
  }

  /**
   * Called when individual test starts
   */
  onTestStart(test) {
    const testKey = `${test.parent.path}::${test.name}`;

    this.metrics.testMetrics[testKey] = {
      name: test.name,
      suite: this.getSuiteName(test.parent.path),
      startTime: Date.now(),
      endTime: null,
      duration: 0,
      status: 'running',
      errors: [],
      retries: 0,
    };
  }

  /**
   * Called when individual test completes
   */
  onTestResult(test, testResult, aggregatedResult) {
    const suitePath = test.path;
    const suiteMetrics = this.metrics.suiteMetrics[suitePath];

    if (!suiteMetrics) {
      console.warn(`⚠️  No metrics found for suite: ${suitePath}`);
      return;
    }

    // Update suite end time and duration
    suiteMetrics.endTime = Date.now();
    suiteMetrics.duration = suiteMetrics.endTime - suiteMetrics.startTime;

    // Update test counts
    suiteMetrics.tests.total = testResult.testResults.length;
    suiteMetrics.tests.passed = testResult.numPassingTests;
    suiteMetrics.tests.failed = testResult.numFailingTests;
    suiteMetrics.tests.skipped = testResult.numPendingTests;
    suiteMetrics.tests.todo = testResult.numTodoTests;

    // Update individual test metrics
    testResult.testResults.forEach(result => {
      const testKey = `${suitePath}::${result.title}`;
      const testMetrics = this.metrics.testMetrics[testKey];

      if (testMetrics) {
        testMetrics.endTime = Date.now();
        testMetrics.duration = testMetrics.endTime - testMetrics.startTime;
        testMetrics.status = result.status;

        if (result.failureMessages.length > 0) {
          testMetrics.errors = result.failureMessages;
        }
      }
    });

    // Calculate performance metrics
    this.calculateSuitePerformanceMetrics(suiteMetrics, testResult);

    // Update coverage if available
    if (testResult.coverage) {
      this.updateCoverageMetrics(suiteMetrics, testResult.coverage);
    }

    // Collect errors and warnings
    if (testResult.numFailingTests > 0) {
      this.testState.consecutiveFailures++;
      suiteMetrics.errors = testResult.testResults
        .filter(test => test.status === 'failed')
        .map(test => ({
          testName: test.title,
          errors: test.failureMessages,
        }));
    } else {
      this.testState.consecutiveFailures = 0;
    }

    // Update progress
    this.testState.testsCompleted++;
    const progress = Math.round(
      (this.testState.testsCompleted / this.testState.testsTotal) * 100
    );

    // Report suite completion
    const status = testResult.numFailingTests > 0 ? '❌' : '✅';
    console.log(
      `${status} ${suiteMetrics.name}: ${suiteMetrics.tests.passed}/${suiteMetrics.tests.total} passed (${suiteMetrics.duration}ms) [${progress}%]`
    );

    // Check for alerts
    this.checkForAlerts(suiteMetrics, testResult);

    // Collect performance data
    if (this.options.trackPerformance) {
      this.collectPerformanceData(suiteMetrics, testResult);
    }
  }

  /**
   * Called when test run completes
   */
  onRunComplete(contexts, results) {
    this.metrics.endTime = new Date();
    this.metrics.totalDuration =
      this.metrics.endTime.getTime() - this.metrics.startTime.getTime();

    // Generate final summary
    this.generateSummary(results);

    // Generate trends data if enabled
    if (this.options.generateTrends) {
      this.generateTrendsData();
    }

    // Write metrics to file
    this.writeMetricsToFile();

    // Print final report
    this.printFinalReport(results);

    // Return reporter result
    return Promise.resolve({
      success: results.numFailedTestSuites === 0,
      metrics: this.metrics,
      alerts: this.metrics.alerts,
    });
  }

  /**
   * Calculate performance metrics for test suite
   */
  calculateSuitePerformanceMetrics(suiteMetrics, testResult) {
    const testDurations = testResult.testResults.map(
      test => test.duration || 0
    );

    if (testDurations.length > 0) {
      // Average test duration
      suiteMetrics.performance.avgTestDuration =
        testDurations.reduce((sum, duration) => sum + duration, 0) /
        testDurations.length;

      // Slowest and fastest tests
      const maxDuration = Math.max(...testDurations);
      const minDuration = Math.min(...testDurations);

      const slowestTest = testResult.testResults.find(
        test => (test.duration || 0) === maxDuration
      );
      const fastestTest = testResult.testResults.find(
        test => (test.duration || 0) === minDuration
      );

      if (slowestTest) {
        suiteMetrics.performance.slowestTest = {
          name: slowestTest.title,
          duration: maxDuration,
        };
      }

      if (fastestTest) {
        suiteMetrics.performance.fastestTest = {
          name: fastestTest.title,
          duration: minDuration,
        };
      }
    }

    // Memory usage (if available)
    if (testResult.perfStats && testResult.perfStats.runtime) {
      suiteMetrics.performance.memoryUsage = {
        heapUsed: process.memoryUsage().heapUsed,
        heapTotal: process.memoryUsage().heapTotal,
        external: process.memoryUsage().external,
        runtime: testResult.perfStats.runtime,
      };
    }
  }

  /**
   * Update coverage metrics
   */
  updateCoverageMetrics(suiteMetrics, coverage) {
    if (coverage.global) {
      suiteMetrics.coverage = {
        statements: coverage.global.statements.pct || 0,
        branches: coverage.global.branches.pct || 0,
        functions: coverage.global.functions.pct || 0,
        lines: coverage.global.lines.pct || 0,
      };
    }
  }

  /**
   * Check for performance and quality alerts
   */
  checkForAlerts(suiteMetrics, testResult) {
    const alerts = [];

    // Success rate alert
    const successRate =
      (suiteMetrics.tests.passed / suiteMetrics.tests.total) * 100;
    if (successRate < this.options.alertThresholds.successRate) {
      alerts.push({
        type: 'success_rate',
        severity: 'high',
        message: `Success rate (${successRate.toFixed(1)}%) below threshold (${this.options.alertThresholds.successRate}%)`,
        suite: suiteMetrics.name,
        value: successRate,
        threshold: this.options.alertThresholds.successRate,
      });
    }

    // Duration alert
    if (suiteMetrics.duration > this.options.alertThresholds.avgDuration) {
      alerts.push({
        type: 'duration',
        severity: 'medium',
        message: `Suite duration (${suiteMetrics.duration}ms) above threshold (${this.options.alertThresholds.avgDuration}ms)`,
        suite: suiteMetrics.name,
        value: suiteMetrics.duration,
        threshold: this.options.alertThresholds.avgDuration,
      });
    }

    // Consecutive failures alert
    if (this.testState.consecutiveFailures >= 3) {
      alerts.push({
        type: 'consecutive_failures',
        severity: 'critical',
        message: `${this.testState.consecutiveFailures} consecutive test failures detected`,
        suite: suiteMetrics.name,
        value: this.testState.consecutiveFailures,
        threshold: 3,
      });
    }

    // Memory usage alert (if available)
    if (suiteMetrics.performance.memoryUsage) {
      const heapUsagePercent =
        (suiteMetrics.performance.memoryUsage.heapUsed /
          suiteMetrics.performance.memoryUsage.heapTotal) *
        100;
      if (heapUsagePercent > 85) {
        alerts.push({
          type: 'memory_usage',
          severity: 'medium',
          message: `High memory usage detected (${heapUsagePercent.toFixed(1)}%)`,
          suite: suiteMetrics.name,
          value: heapUsagePercent,
          threshold: 85,
        });
      }
    }

    // Add alerts to metrics
    this.metrics.alerts.push(...alerts);

    // Print alerts immediately
    alerts.forEach(alert => {
      const emoji =
        alert.severity === 'critical'
          ? '🚨'
          : alert.severity === 'high'
            ? '⚠️'
            : '⚡';
      console.log(
        `${emoji} ALERT [${alert.severity.toUpperCase()}]: ${alert.message}`
      );
    });
  }

  /**
   * Collect performance data for trending
   */
  collectPerformanceData(suiteMetrics, testResult) {
    this.metrics.performanceData.push({
      timestamp: Date.now(),
      suite: suiteMetrics.name,
      duration: suiteMetrics.duration,
      avgTestDuration: suiteMetrics.performance.avgTestDuration,
      successRate: (suiteMetrics.tests.passed / suiteMetrics.tests.total) * 100,
      testCount: suiteMetrics.tests.total,
      memoryUsage: suiteMetrics.performance.memoryUsage?.heapUsed || null,
    });
  }

  /**
   * Generate comprehensive summary
   */
  generateSummary(results) {
    const totalSuites = Object.keys(this.metrics.suiteMetrics).length;
    const totalTests = Object.values(this.metrics.suiteMetrics).reduce(
      (sum, suite) => sum + suite.tests.total,
      0
    );
    const passedTests = Object.values(this.metrics.suiteMetrics).reduce(
      (sum, suite) => sum + suite.tests.passed,
      0
    );
    const failedTests = Object.values(this.metrics.suiteMetrics).reduce(
      (sum, suite) => sum + suite.tests.failed,
      0
    );

    const overallSuccessRate =
      totalTests > 0 ? (passedTests / totalTests) * 100 : 0;
    const avgSuiteDuration =
      totalSuites > 0
        ? Object.values(this.metrics.suiteMetrics).reduce(
            (sum, suite) => sum + suite.duration,
            0
          ) / totalSuites
        : 0;

    this.metrics.summary = {
      execution: {
        totalDuration: this.metrics.totalDuration,
        totalSuites,
        totalTests,
        passedTests,
        failedTests,
        skippedTests: results.numPendingTests,
        successRate: overallSuccessRate,
        avgSuiteDuration,
      },
      performance: {
        slowestSuite: this.findSlowestSuite(),
        fastestSuite: this.findFastestSuite(),
        averageTestDuration: this.calculateOverallAvgTestDuration(),
        performanceGrade: this.calculatePerformanceGrade(
          overallSuccessRate,
          avgSuiteDuration
        ),
      },
      quality: {
        alertCount: this.metrics.alerts.length,
        criticalAlerts: this.metrics.alerts.filter(
          a => a.severity === 'critical'
        ).length,
        highAlerts: this.metrics.alerts.filter(a => a.severity === 'high')
          .length,
        qualityGrade: this.calculateQualityGrade(
          overallSuccessRate,
          this.metrics.alerts.length
        ),
      },
      coverage: this.calculateOverallCoverage(),
    };
  }

  /**
   * Generate trends data for historical analysis
   */
  generateTrendsData() {
    const trendsFile = path.join(
      path.dirname(this.options.outputFile),
      'trends.json'
    );
    let trendsHistory = [];

    // Load existing trends data
    try {
      if (fs.existsSync(trendsFile)) {
        trendsHistory = JSON.parse(fs.readFileSync(trendsFile, 'utf8'));
      }
    } catch (error) {
      console.warn('Failed to load trends history:', error.message);
    }

    // Add current execution to trends
    trendsHistory.push({
      timestamp: this.metrics.startTime.toISOString(),
      duration: this.metrics.totalDuration,
      successRate: this.metrics.summary.execution.successRate,
      testCount: this.metrics.summary.execution.totalTests,
      suiteCount: this.metrics.summary.execution.totalSuites,
      alertCount: this.metrics.summary.quality.alertCount,
      avgSuiteDuration: this.metrics.summary.execution.avgSuiteDuration,
      performanceGrade: this.metrics.summary.performance.performanceGrade,
      qualityGrade: this.metrics.summary.quality.qualityGrade,
    });

    // Keep only last 50 executions for trends
    if (trendsHistory.length > 50) {
      trendsHistory = trendsHistory.slice(-50);
    }

    // Write updated trends
    try {
      fs.writeFileSync(trendsFile, JSON.stringify(trendsHistory, null, 2));
    } catch (error) {
      console.warn('Failed to write trends data:', error.message);
    }

    this.metrics.trendsData = trendsHistory;
  }

  /**
   * Write all metrics to output file
   */
  writeMetricsToFile() {
    try {
      const metricsData = {
        timestamp: this.metrics.startTime.toISOString(),
        automation: {
          version: '1.0.0',
          reporter: 'IntegrationAutomationReporter',
          configuration: this.options,
        },
        execution: this.metrics,
        trends: this.metrics.trendsData,
      };

      fs.writeFileSync(
        this.options.outputFile,
        JSON.stringify(metricsData, null, 2)
      );
      console.log(
        `📄 Automation metrics written to: ${this.options.outputFile}`
      );
    } catch (error) {
      console.error('Failed to write metrics file:', error.message);
    }
  }

  /**
   * Print comprehensive final report
   */
  printFinalReport(results) {
    console.log('\n' + '='.repeat(80));
    console.log('🎯 INTEGRATION TEST AUTOMATION REPORT');
    console.log('='.repeat(80));

    // Execution Summary
    console.log('\n📊 EXECUTION SUMMARY');
    console.log('-'.repeat(40));
    console.log(
      `⏱️  Total Duration: ${(this.metrics.totalDuration / 1000).toFixed(1)}s`
    );
    console.log(
      `📁 Test Suites: ${this.metrics.summary.execution.totalSuites}`
    );
    console.log(`🧪 Total Tests: ${this.metrics.summary.execution.totalTests}`);
    console.log(`✅ Passed: ${this.metrics.summary.execution.passedTests}`);
    console.log(`❌ Failed: ${this.metrics.summary.execution.failedTests}`);
    console.log(`⏭️  Skipped: ${this.metrics.summary.execution.skippedTests}`);
    console.log(
      `🎯 Success Rate: ${this.metrics.summary.execution.successRate.toFixed(1)}%`
    );

    // Performance Summary
    console.log('\n⚡ PERFORMANCE SUMMARY');
    console.log('-'.repeat(40));
    console.log(
      `📈 Performance Grade: ${this.metrics.summary.performance.performanceGrade}`
    );
    console.log(
      `⏱️  Avg Suite Duration: ${(this.metrics.summary.execution.avgSuiteDuration / 1000).toFixed(1)}s`
    );
    console.log(
      `⏱️  Avg Test Duration: ${(this.metrics.summary.performance.averageTestDuration / 1000).toFixed(1)}s`
    );

    if (this.metrics.summary.performance.slowestSuite) {
      console.log(
        `🐌 Slowest Suite: ${this.metrics.summary.performance.slowestSuite.name} (${(this.metrics.summary.performance.slowestSuite.duration / 1000).toFixed(1)}s)`
      );
    }

    // Quality Summary
    console.log('\n🎖️  QUALITY SUMMARY');
    console.log('-'.repeat(40));
    console.log(
      `🏆 Quality Grade: ${this.metrics.summary.quality.qualityGrade}`
    );
    console.log(`⚠️  Total Alerts: ${this.metrics.summary.quality.alertCount}`);
    console.log(`🚨 Critical: ${this.metrics.summary.quality.criticalAlerts}`);
    console.log(`⚠️  High: ${this.metrics.summary.quality.highAlerts}`);

    // Coverage Summary (if available)
    if (
      this.metrics.summary.coverage &&
      this.metrics.summary.coverage.statements > 0
    ) {
      console.log('\n📋 COVERAGE SUMMARY');
      console.log('-'.repeat(40));
      console.log(
        `📝 Statements: ${this.metrics.summary.coverage.statements.toFixed(1)}%`
      );
      console.log(
        `🌿 Branches: ${this.metrics.summary.coverage.branches.toFixed(1)}%`
      );
      console.log(
        `🔧 Functions: ${this.metrics.summary.coverage.functions.toFixed(1)}%`
      );
      console.log(
        `📏 Lines: ${this.metrics.summary.coverage.lines.toFixed(1)}%`
      );
    }

    // Recent Alerts
    if (this.metrics.alerts.length > 0) {
      console.log('\n🚨 RECENT ALERTS');
      console.log('-'.repeat(40));
      this.metrics.alerts.slice(-5).forEach(alert => {
        const emoji =
          alert.severity === 'critical'
            ? '🚨'
            : alert.severity === 'high'
              ? '⚠️'
              : '⚡';
        console.log(
          `${emoji} [${alert.severity.toUpperCase()}] ${alert.message}`
        );
      });
    }

    console.log('\n' + '='.repeat(80));
    console.log(
      `🎉 Integration Test Automation Complete! Success Rate: ${this.metrics.summary.execution.successRate.toFixed(1)}%`
    );
    console.log('='.repeat(80) + '\n');
  }

  // Helper methods
  getSuiteName(filePath) {
    return path.basename(filePath, path.extname(filePath));
  }

  findSlowestSuite() {
    const suites = Object.values(this.metrics.suiteMetrics);
    return suites.reduce(
      (slowest, suite) =>
        !slowest || suite.duration > slowest.duration
          ? { name: suite.name, duration: suite.duration }
          : slowest,
      null
    );
  }

  findFastestSuite() {
    const suites = Object.values(this.metrics.suiteMetrics);
    return suites.reduce(
      (fastest, suite) =>
        !fastest || suite.duration < fastest.duration
          ? { name: suite.name, duration: suite.duration }
          : fastest,
      null
    );
  }

  calculateOverallAvgTestDuration() {
    const allTests = Object.values(this.metrics.testMetrics);
    if (allTests.length === 0) return 0;

    return (
      allTests.reduce((sum, test) => sum + test.duration, 0) / allTests.length
    );
  }

  calculatePerformanceGrade(successRate, avgDuration) {
    if (successRate >= 98 && avgDuration <= 30000) return 'A+';
    if (successRate >= 95 && avgDuration <= 45000) return 'A';
    if (successRate >= 90 && avgDuration <= 60000) return 'B';
    if (successRate >= 80 && avgDuration <= 90000) return 'C';
    return 'D';
  }

  calculateQualityGrade(successRate, alertCount) {
    if (successRate >= 98 && alertCount === 0) return 'A+';
    if (successRate >= 95 && alertCount <= 2) return 'A';
    if (successRate >= 90 && alertCount <= 5) return 'B';
    if (successRate >= 80 && alertCount <= 10) return 'C';
    return 'D';
  }

  calculateOverallCoverage() {
    const suites = Object.values(this.metrics.suiteMetrics);
    const coverageSuites = suites.filter(
      suite => suite.coverage.statements > 0
    );

    if (coverageSuites.length === 0) return null;

    return {
      statements:
        coverageSuites.reduce(
          (sum, suite) => sum + suite.coverage.statements,
          0
        ) / coverageSuites.length,
      branches:
        coverageSuites.reduce(
          (sum, suite) => sum + suite.coverage.branches,
          0
        ) / coverageSuites.length,
      functions:
        coverageSuites.reduce(
          (sum, suite) => sum + suite.coverage.functions,
          0
        ) / coverageSuites.length,
      lines:
        coverageSuites.reduce((sum, suite) => sum + suite.coverage.lines, 0) /
        coverageSuites.length,
    };
  }
}

module.exports = IntegrationAutomationReporter;
