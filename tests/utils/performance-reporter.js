/**
 * Performance Reporter for Jest Tests
 * Tracks test execution performance and identifies bottlenecks
 */

const fs = require('fs').promises;
const path = require('path');

class PerformanceReporter {
  constructor(globalConfig, options = {}) {
    this.globalConfig = globalConfig;
    this.options = {
      outputPath:
        options.outputPath || './tests/results/performance-metrics.json',
      thresholds: {
        maxTestTime: options.thresholds?.maxTestTime || 5000, // 5 seconds
        maxSuiteTime: options.thresholds?.maxSuiteTime || 60000, // 1 minute
        maxSlowTests: 10,
        maxSlowSuites: 5,
      },
      ...options,
    };

    this.startTime = Date.now();
    this.testMetrics = [];
    this.suiteMetrics = [];
    this.memoryUsage = [];
  }

  /**
   * Called when a test suite starts
   */
  onRunStart() {
    this.startTime = Date.now();
    this.recordMemoryUsage('run_start');
    console.log('🚀 Performance monitoring started...');
  }

  /**
   * Called when a test file starts
   */
  onTestFileStart(test) {
    this.recordMemoryUsage(`suite_start_${path.basename(test.path)}`);
  }

  /**
   * Called when a test file completes
   */
  onTestFileResult(test, testResult) {
    const suiteMetric = {
      path: testResult.testFilePath,
      duration: testResult.perfStats
        ? testResult.perfStats.end - testResult.perfStats.start
        : 0,
      numTests: testResult.numPassingTests + testResult.numFailingTests,
      memoryUsage: process.memoryUsage(),
      timestamp: Date.now(),
      slow: false,
    };

    // Mark slow suites
    if (suiteMetric.duration > this.options.thresholds.maxSuiteTime) {
      suiteMetric.slow = true;
      suiteMetric.warning = `Suite exceeded ${this.options.thresholds.maxSuiteTime}ms threshold`;
    }

    this.suiteMetrics.push(suiteMetric);
    this.recordMemoryUsage(
      `suite_end_${path.basename(testResult.testFilePath)}`
    );

    // Track individual test performance
    if (testResult.assertionResults) {
      testResult.assertionResults.forEach(assertion => {
        const testMetric = {
          suitePath: testResult.testFilePath,
          testName: assertion.fullName,
          duration: assertion.duration || 0,
          status: assertion.status,
          timestamp: Date.now(),
          slow: false,
        };

        // Mark slow tests
        if (testMetric.duration > this.options.thresholds.maxTestTime) {
          testMetric.slow = true;
          testMetric.warning = `Test exceeded ${this.options.thresholds.maxTestTime}ms threshold`;
        }

        this.testMetrics.push(testMetric);
      });
    }
  }

  /**
   * Called when all tests complete
   */
  async onRunComplete(testSuites, results) {
    this.recordMemoryUsage('run_end');

    const endTime = Date.now();
    const totalDuration = endTime - this.startTime;

    const performanceReport = {
      timestamp: new Date().toISOString(),
      summary: {
        totalDuration,
        totalTests: results.numTotalTests,
        totalSuites: testSuites.length,
        avgTestTime: this.calculateAverageTestTime(),
        avgSuiteTime: this.calculateAverageSuiteTime(),
        parallelEfficiency: this.calculateParallelEfficiency(totalDuration),
        memoryEfficiency: this.calculateMemoryEfficiency(),
      },
      thresholds: this.options.thresholds,
      violations: this.identifyViolations(),
      slowTests: this.getSlowTests(),
      slowSuites: this.getSlowSuites(),
      memoryProfile: this.memoryUsage,
      recommendations: this.generateRecommendations(),
      suiteBreakdown: this.categorizeSuitePerformance(),
      trends: await this.comparePreviousRuns(),
    };

    // Save report
    await this.saveReport(performanceReport);

    // Print summary
    this.printPerformanceSummary(performanceReport);

    return performanceReport;
  }

  /**
   * Record memory usage at specific points
   */
  recordMemoryUsage(checkpoint) {
    const usage = process.memoryUsage();
    this.memoryUsage.push({
      checkpoint,
      timestamp: Date.now(),
      rss: usage.rss,
      heapTotal: usage.heapTotal,
      heapUsed: usage.heapUsed,
      external: usage.external,
    });
  }

  /**
   * Calculate average test execution time
   */
  calculateAverageTestTime() {
    if (this.testMetrics.length === 0) return 0;

    const totalTime = this.testMetrics.reduce(
      (sum, test) => sum + test.duration,
      0
    );
    return totalTime / this.testMetrics.length;
  }

  /**
   * Calculate average suite execution time
   */
  calculateAverageSuiteTime() {
    if (this.suiteMetrics.length === 0) return 0;

    const totalTime = this.suiteMetrics.reduce(
      (sum, suite) => sum + suite.duration,
      0
    );
    return totalTime / this.suiteMetrics.length;
  }

  /**
   * Calculate parallel execution efficiency
   */
  calculateParallelEfficiency(totalDuration) {
    const sequentialTime = this.suiteMetrics.reduce(
      (sum, suite) => sum + suite.duration,
      0
    );

    if (sequentialTime === 0) return 1;

    const efficiency = sequentialTime / totalDuration;
    return Math.min(efficiency, 1); // Cap at 100%
  }

  /**
   * Calculate memory usage efficiency
   */
  calculateMemoryEfficiency() {
    if (this.memoryUsage.length < 2)
      return { stable: true, peakMB: 0, growthMB: 0 };

    const peakHeap = Math.max(...this.memoryUsage.map(m => m.heapUsed));
    const initialHeap = this.memoryUsage[0].heapUsed;
    const finalHeap = this.memoryUsage[this.memoryUsage.length - 1].heapUsed;

    return {
      peakMB: Math.round(peakHeap / 1024 / 1024),
      growthMB: Math.round((finalHeap - initialHeap) / 1024 / 1024),
      stable: finalHeap - initialHeap < 50 * 1024 * 1024, // 50MB growth threshold
      leakSuspected: finalHeap - initialHeap > 200 * 1024 * 1024, // 200MB growth
    };
  }

  /**
   * Identify performance threshold violations
   */
  identifyViolations() {
    const violations = [];

    // Slow tests
    const slowTests = this.testMetrics.filter(test => test.slow);
    if (slowTests.length > this.options.thresholds.maxSlowTests) {
      violations.push({
        type: 'slow_tests',
        count: slowTests.length,
        threshold: this.options.thresholds.maxSlowTests,
        severity: 'medium',
      });
    }

    // Slow suites
    const slowSuites = this.suiteMetrics.filter(suite => suite.slow);
    if (slowSuites.length > this.options.thresholds.maxSlowSuites) {
      violations.push({
        type: 'slow_suites',
        count: slowSuites.length,
        threshold: this.options.thresholds.maxSlowSuites,
        severity: 'high',
      });
    }

    // Memory issues
    const memEfficiency = this.calculateMemoryEfficiency();
    if (memEfficiency.leakSuspected) {
      violations.push({
        type: 'memory_leak',
        growthMB: memEfficiency.growthMB,
        severity: 'high',
      });
    }

    // Low parallel efficiency
    const parallelEfficiency = this.calculateParallelEfficiency(
      Date.now() - this.startTime
    );
    if (parallelEfficiency < 0.3) {
      violations.push({
        type: 'low_parallelization',
        efficiency: Math.round(parallelEfficiency * 100),
        severity: 'medium',
      });
    }

    return violations;
  }

  /**
   * Get slowest tests
   */
  getSlowTests() {
    return this.testMetrics
      .sort((a, b) => b.duration - a.duration)
      .slice(0, 10)
      .map(test => ({
        name: test.testName,
        suite: path.basename(test.suitePath),
        duration: test.duration,
        slow: test.slow,
      }));
  }

  /**
   * Get slowest suites
   */
  getSlowSuites() {
    return this.suiteMetrics
      .sort((a, b) => b.duration - a.duration)
      .slice(0, 5)
      .map(suite => ({
        name: path.basename(suite.path),
        path: suite.path,
        duration: suite.duration,
        numTests: suite.numTests,
        slow: suite.slow,
      }));
  }

  /**
   * Categorize suite performance by type
   */
  categorizeSuitePerformance() {
    const categories = {
      unit: { count: 0, totalTime: 0, avgTime: 0 },
      integration: { count: 0, totalTime: 0, avgTime: 0 },
      security: { count: 0, totalTime: 0, avgTime: 0 },
      frontend: { count: 0, totalTime: 0, avgTime: 0 },
      smartContracts: { count: 0, totalTime: 0, avgTime: 0 },
    };

    this.suiteMetrics.forEach(suite => {
      let category = 'unit'; // default

      if (suite.path.includes('/integration/')) category = 'integration';
      else if (suite.path.includes('/security/')) category = 'security';
      else if (suite.path.includes('/frontend/') || suite.path.includes('.jsx'))
        category = 'frontend';
      else if (suite.path.includes('/smart-contracts/'))
        category = 'smartContracts';

      categories[category].count++;
      categories[category].totalTime += suite.duration;
    });

    // Calculate averages
    Object.keys(categories).forEach(category => {
      const cat = categories[category];
      cat.avgTime = cat.count > 0 ? cat.totalTime / cat.count : 0;
    });

    return categories;
  }

  /**
   * Generate performance recommendations
   */
  generateRecommendations() {
    const recommendations = [];
    const violations = this.identifyViolations();

    // Slow tests recommendation
    const slowTestViolation = violations.find(v => v.type === 'slow_tests');
    if (slowTestViolation) {
      recommendations.push({
        category: 'Test Performance',
        priority: 'medium',
        issue: `${slowTestViolation.count} slow tests detected`,
        action:
          'Consider mocking external dependencies, reducing test complexity, or splitting into smaller tests',
        affectedTests: this.getSlowTests()
          .slice(0, 5)
          .map(t => t.name),
      });
    }

    // Slow suites recommendation
    const slowSuiteViolation = violations.find(v => v.type === 'slow_suites');
    if (slowSuiteViolation) {
      recommendations.push({
        category: 'Suite Performance',
        priority: 'high',
        issue: `${slowSuiteViolation.count} slow test suites detected`,
        action:
          'Optimize test setup/teardown, use test database optimization, or increase parallelization',
        affectedSuites: this.getSlowSuites()
          .slice(0, 3)
          .map(s => s.name),
      });
    }

    // Memory recommendation
    const memoryViolation = violations.find(v => v.type === 'memory_leak');
    if (memoryViolation) {
      recommendations.push({
        category: 'Memory Usage',
        priority: 'high',
        issue: `Suspected memory leak: ${memoryViolation.growthMB}MB growth`,
        action:
          'Check for unclosed connections, large object retention, or missing cleanup in teardown',
      });
    }

    // Parallelization recommendation
    const parallelViolation = violations.find(
      v => v.type === 'low_parallelization'
    );
    if (parallelViolation) {
      recommendations.push({
        category: 'Parallelization',
        priority: 'medium',
        issue: `Low parallel efficiency: ${parallelViolation.efficiency}%`,
        action:
          'Review maxWorkers configuration, reduce resource contention, or optimize test isolation',
      });
    }

    return recommendations;
  }

  /**
   * Compare with previous runs for trend analysis
   */
  async comparePreviousRuns() {
    try {
      const historyPath = this.options.outputPath.replace(
        '.json',
        '-history.json'
      );

      let history = [];
      try {
        const data = await fs.readFile(historyPath, 'utf8');
        history = JSON.parse(data);
      } catch (error) {
        // No history file exists yet
      }

      // Keep last 10 runs
      if (history.length >= 10) {
        history = history.slice(-9);
      }

      const currentMetrics = {
        timestamp: new Date().toISOString(),
        avgTestTime: this.calculateAverageTestTime(),
        avgSuiteTime: this.calculateAverageSuiteTime(),
        totalTests: this.testMetrics.length,
        slowTests: this.testMetrics.filter(t => t.slow).length,
        memoryPeakMB: this.calculateMemoryEfficiency().peakMB,
      };

      history.push(currentMetrics);

      // Save updated history
      await fs.writeFile(historyPath, JSON.stringify(history, null, 2));

      // Calculate trends
      if (history.length >= 2) {
        const previous = history[history.length - 2];
        const current = currentMetrics;

        return {
          avgTestTimeChange: current.avgTestTime - previous.avgTestTime,
          avgSuiteTimeChange: current.avgSuiteTime - previous.avgSuiteTime,
          slowTestsChange: current.slowTests - previous.slowTests,
          memoryChange: current.memoryPeakMB - previous.memoryPeakMB,
          improving:
            current.avgTestTime <= previous.avgTestTime &&
            current.slowTests <= previous.slowTests,
        };
      }

      return { firstRun: true };
    } catch (error) {
      console.warn('Could not analyze trends:', error.message);
      return { error: error.message };
    }
  }

  /**
   * Save performance report
   */
  async saveReport(report) {
    try {
      // Ensure directory exists
      const dir = path.dirname(this.options.outputPath);
      await fs.mkdir(dir, { recursive: true });

      // Save report
      await fs.writeFile(
        this.options.outputPath,
        JSON.stringify(report, null, 2)
      );
    } catch (error) {
      console.error('Failed to save performance report:', error.message);
    }
  }

  /**
   * Print performance summary to console
   */
  printPerformanceSummary(report) {
    console.log('\n⚡ Test Performance Summary');
    console.log('=' * 40);

    console.log(
      `Total Duration: ${Math.round(report.summary.totalDuration)}ms`
    );
    console.log(
      `Average Test Time: ${Math.round(report.summary.avgTestTime)}ms`
    );
    console.log(
      `Average Suite Time: ${Math.round(report.summary.avgSuiteTime)}ms`
    );
    console.log(
      `Parallel Efficiency: ${Math.round(report.summary.parallelEfficiency * 100)}%`
    );
    console.log(`Memory Peak: ${report.summary.memoryEfficiency.peakMB}MB`);

    if (report.violations.length > 0) {
      console.log('\n⚠️  Performance Issues:');
      report.violations.forEach(violation => {
        console.log(
          `  • ${violation.type}: ${this.formatViolation(violation)}`
        );
      });
    }

    if (report.slowTests.length > 0) {
      console.log('\n🐌 Slowest Tests:');
      report.slowTests.slice(0, 3).forEach(test => {
        console.log(`  • ${test.name}: ${test.duration}ms`);
      });
    }

    if (report.recommendations.length > 0) {
      console.log('\n💡 Performance Recommendations:');
      report.recommendations.slice(0, 2).forEach(rec => {
        console.log(`  • [${rec.priority.toUpperCase()}] ${rec.issue}`);
        console.log(`    Action: ${rec.action}`);
      });
    }

    console.log(`\n📄 Full report: ${this.options.outputPath}`);
  }

  /**
   * Format violation for display
   */
  formatViolation(violation) {
    switch (violation.type) {
      case 'slow_tests':
        return `${violation.count} tests exceed ${this.options.thresholds.maxTestTime}ms`;
      case 'slow_suites':
        return `${violation.count} suites exceed ${this.options.thresholds.maxSuiteTime}ms`;
      case 'memory_leak':
        return `${violation.growthMB}MB memory growth suspected`;
      case 'low_parallelization':
        return `${violation.efficiency}% efficiency (poor parallelization)`;
      default:
        return JSON.stringify(violation);
    }
  }
}

module.exports = PerformanceReporter;
