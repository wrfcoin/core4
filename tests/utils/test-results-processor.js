/**
 * Test Results Processor for CI/CD Pipeline
 * Aggregates and analyzes test results from multiple sources
 */

const fs = require('fs').promises;
const path = require('path');

class TestResultsProcessor {
  constructor(options = {}) {
    this.resultsDir =
      options.resultsDir || path.join(process.cwd(), 'tests/results');
    this.outputPath =
      options.outputPath ||
      path.join(this.resultsDir, 'aggregated-results.json');
    this.thresholds = options.thresholds || {
      coverage: { min: 80 },
      performance: { maxLoadTime: 3000 },
      security: { maxCritical: 0, maxHigh: 2 },
      quality: { minScore: 8.0 },
    };
  }

  /**
   * Process Jest test results
   */
  async processJestResults() {
    try {
      // Jest JSON reporter output
      const jestResultsPath = path.join(this.resultsDir, 'jest-results.json');
      let jestData = null;

      try {
        const data = await fs.readFile(jestResultsPath, 'utf8');
        jestData = JSON.parse(data);
      } catch (error) {
        console.warn('Jest results not found, checking junit.xml');
        // Fallback to junit XML parsing if needed
        jestData = await this.parseJunitXml();
      }

      if (!jestData) return null;

      return {
        type: 'jest',
        timestamp: new Date().toISOString(),
        summary: {
          totalTests: jestData.numTotalTests || 0,
          passedTests: jestData.numPassedTests || 0,
          failedTests: jestData.numFailedTests || 0,
          skippedTests: jestData.numPendingTests || 0,
          duration: jestData.testResults
            ? jestData.testResults.reduce(
                (sum, result) =>
                  sum + (result.perfStats?.end - result.perfStats?.start || 0),
                0
              )
            : 0,
          success: jestData.success || false,
        },
        coverage: await this.extractCoverageData(),
        failures: this.extractTestFailures(jestData),
        performance: this.extractTestPerformance(jestData),
        suites: this.categorizeTestSuites(jestData),
      };
    } catch (error) {
      console.error('Failed to process Jest results:', error.message);
      return null;
    }
  }

  /**
   * Parse JUnit XML as fallback
   */
  async parseJunitXml() {
    try {
      const junitPath = path.join(this.resultsDir, 'junit.xml');
      const xmlData = await fs.readFile(junitPath, 'utf8');

      // Simple XML parsing for test counts
      const totalMatch = xmlData.match(/tests="(\d+)"/);
      const failuresMatch = xmlData.match(/failures="(\d+)"/);
      const errorsMatch = xmlData.match(/errors="(\d+)"/);
      const skippedMatch = xmlData.match(/skipped="(\d+)"/);

      const total = totalMatch ? parseInt(totalMatch[1], 10) : 0;
      const failures = failuresMatch ? parseInt(failuresMatch[1], 10) : 0;
      const errors = errorsMatch ? parseInt(errorsMatch[1], 10) : 0;
      const skipped = skippedMatch ? parseInt(skippedMatch[1], 10) : 0;

      return {
        numTotalTests: total,
        numFailedTests: failures + errors,
        numPassedTests: total - failures - errors - skipped,
        numPendingTests: skipped,
        success: failures + errors === 0,
      };
    } catch (error) {
      console.warn('Could not parse JUnit XML:', error.message);
      return null;
    }
  }

  /**
   * Extract coverage data
   */
  async extractCoverageData() {
    try {
      const coveragePath = path.join(
        this.resultsDir,
        'coverage/coverage-summary.json'
      );
      const data = await fs.readFile(coveragePath, 'utf8');
      const coverage = JSON.parse(data);

      return {
        lines: coverage.total.lines.pct,
        branches: coverage.total.branches.pct,
        functions: coverage.total.functions.pct,
        statements: coverage.total.statements.pct,
        passed: coverage.total.lines.pct >= this.thresholds.coverage.min,
      };
    } catch (error) {
      console.warn('Coverage data not available:', error.message);
      return null;
    }
  }

  /**
   * Extract test failures for analysis
   */
  extractTestFailures(jestData) {
    const failures = [];

    if (jestData.testResults) {
      jestData.testResults.forEach(result => {
        if (result.assertionResults) {
          result.assertionResults.forEach(assertion => {
            if (assertion.status === 'failed') {
              failures.push({
                suite: result.testFilePath,
                test: assertion.fullName,
                error: assertion.failureMessages?.[0],
                duration: assertion.duration,
              });
            }
          });
        }
      });
    }

    return failures;
  }

  /**
   * Extract test performance metrics
   */
  extractTestPerformance(jestData) {
    const performance = {
      slowestTests: [],
      slowestSuites: [],
      avgTestTime: 0,
      totalTime: 0,
    };

    if (jestData.testResults) {
      const testTimes = [];
      const suiteTimes = [];

      jestData.testResults.forEach(result => {
        const suiteTime = result.perfStats
          ? result.perfStats.end - result.perfStats.start
          : 0;

        suiteTimes.push({
          suite: result.testFilePath,
          duration: suiteTime,
        });

        if (result.assertionResults) {
          result.assertionResults.forEach(assertion => {
            if (assertion.duration) {
              testTimes.push({
                test: assertion.fullName,
                suite: result.testFilePath,
                duration: assertion.duration,
              });
            }
          });
        }
      });

      // Sort and get slowest
      performance.slowestTests = testTimes
        .sort((a, b) => b.duration - a.duration)
        .slice(0, 10);

      performance.slowestSuites = suiteTimes
        .sort((a, b) => b.duration - a.duration)
        .slice(0, 5);

      performance.totalTime = suiteTimes.reduce(
        (sum, suite) => sum + suite.duration,
        0
      );
      performance.avgTestTime =
        testTimes.length > 0
          ? testTimes.reduce((sum, test) => sum + test.duration, 0) /
            testTimes.length
          : 0;
    }

    return performance;
  }

  /**
   * Categorize test suites by type
   */
  categorizeTestSuites(jestData) {
    const suites = {
      unit: { total: 0, passed: 0, failed: 0 },
      integration: { total: 0, passed: 0, failed: 0 },
      security: { total: 0, passed: 0, failed: 0 },
      frontend: { total: 0, passed: 0, failed: 0 },
      smartContracts: { total: 0, passed: 0, failed: 0 },
    };

    if (jestData.testResults) {
      jestData.testResults.forEach(result => {
        const filePath = result.testFilePath;
        let category = 'unit'; // default

        if (filePath.includes('/integration/')) category = 'integration';
        else if (filePath.includes('/security/')) category = 'security';
        else if (filePath.includes('/frontend/') || filePath.includes('.jsx'))
          category = 'frontend';
        else if (filePath.includes('/smart-contracts/'))
          category = 'smartContracts';

        const passed = result.numPassingTests || 0;
        const failed = result.numFailingTests || 0;
        const total = passed + failed;

        suites[category].total += total;
        suites[category].passed += passed;
        suites[category].failed += failed;
      });
    }

    return suites;
  }

  /**
   * Process Lighthouse CI results
   */
  async processLighthouseResults() {
    try {
      const lighthousePath = path.join(this.resultsDir, 'lighthouse');
      const files = await fs.readdir(lighthousePath);
      const reportFiles = files.filter(
        f => f.endsWith('.json') && !f.includes('manifest')
      );

      if (reportFiles.length === 0) return null;

      const results = [];
      for (const file of reportFiles) {
        const filePath = path.join(lighthousePath, file);
        const data = await fs.readFile(filePath, 'utf8');
        const report = JSON.parse(data);

        results.push({
          url: report.finalUrl,
          scores: {
            performance: Math.round(report.categories.performance.score * 100),
            accessibility: Math.round(
              report.categories.accessibility.score * 100
            ),
            bestPractices: Math.round(
              report.categories['best-practices'].score * 100
            ),
            seo: Math.round(report.categories.seo.score * 100),
            pwa: report.categories.pwa
              ? Math.round(report.categories.pwa.score * 100)
              : null,
          },
          metrics: {
            firstContentfulPaint:
              report.audits['first-contentful-paint'].numericValue,
            largestContentfulPaint:
              report.audits['largest-contentful-paint'].numericValue,
            speedIndex: report.audits['speed-index'].numericValue,
            timeToInteractive: report.audits['interactive'].numericValue,
            totalBlockingTime:
              report.audits['total-blocking-time'].numericValue,
            cumulativeLayoutShift:
              report.audits['cumulative-layout-shift'].numericValue,
          },
          passed: report.categories.performance.score >= 0.8,
        });
      }

      return {
        type: 'lighthouse',
        timestamp: new Date().toISOString(),
        summary: {
          totalPages: results.length,
          passedPages: results.filter(r => r.passed).length,
          avgPerformance: Math.round(
            results.reduce((sum, r) => sum + r.scores.performance, 0) /
              results.length
          ),
          avgAccessibility: Math.round(
            results.reduce((sum, r) => sum + r.scores.accessibility, 0) /
              results.length
          ),
        },
        results,
        violations: this.findLighthouseViolations(results),
      };
    } catch (error) {
      console.warn('Lighthouse results not available:', error.message);
      return null;
    }
  }

  /**
   * Find Lighthouse violations
   */
  findLighthouseViolations(results) {
    const violations = [];

    results.forEach(result => {
      if (result.scores.performance < 80) {
        violations.push({
          type: 'performance',
          url: result.url,
          score: result.scores.performance,
          threshold: 80,
        });
      }

      if (result.metrics.largestContentfulPaint > 2500) {
        violations.push({
          type: 'lcp',
          url: result.url,
          value: result.metrics.largestContentfulPaint,
          threshold: 2500,
        });
      }

      if (result.metrics.cumulativeLayoutShift > 0.1) {
        violations.push({
          type: 'cls',
          url: result.url,
          value: result.metrics.cumulativeLayoutShift,
          threshold: 0.1,
        });
      }
    });

    return violations;
  }

  /**
   * Process security test results
   */
  async processSecurityResults() {
    try {
      // Look for security scan results
      const securityFiles = [
        'security-scan-results.json',
        'sast-results.json',
        'dast-results.json',
        'dependency-check-results.json',
      ];

      const results = {};
      let totalVulnerabilities = { critical: 0, high: 0, medium: 0, low: 0 };

      for (const file of securityFiles) {
        try {
          const filePath = path.join(this.resultsDir, file);
          const data = await fs.readFile(filePath, 'utf8');
          const securityData = JSON.parse(data);

          results[file] = securityData;

          // Aggregate vulnerability counts
          if (securityData.vulnerabilities) {
            securityData.vulnerabilities.forEach(vuln => {
              const severity = vuln.severity?.toLowerCase() || 'low';
              if (totalVulnerabilities[severity] !== undefined) {
                totalVulnerabilities[severity]++;
              }
            });
          }
        } catch (error) {
          // File doesn't exist, skip
        }
      }

      return {
        type: 'security',
        timestamp: new Date().toISOString(),
        summary: {
          totalVulnerabilities: Object.values(totalVulnerabilities).reduce(
            (a, b) => a + b,
            0
          ),
          critical: totalVulnerabilities.critical,
          high: totalVulnerabilities.high,
          medium: totalVulnerabilities.medium,
          low: totalVulnerabilities.low,
          passed:
            totalVulnerabilities.critical <=
              this.thresholds.security.maxCritical &&
            totalVulnerabilities.high <= this.thresholds.security.maxHigh,
        },
        results,
      };
    } catch (error) {
      console.warn('Security results not available:', error.message);
      return null;
    }
  }

  /**
   * Generate quality score based on all metrics
   */
  calculateQualityScore(testResults, coverage, lighthouse, security) {
    let score = 10.0;
    let factors = [];

    // Test success factor (40% weight)
    if (testResults) {
      const testSuccessRate =
        testResults.summary.totalTests > 0
          ? testResults.summary.passedTests / testResults.summary.totalTests
          : 1;
      const testFactor = testSuccessRate * 4.0;
      score = Math.min(score, score * (testFactor / 4.0));
      factors.push({ factor: 'Test Success', weight: 0.4, score: testFactor });
    }

    // Coverage factor (25% weight)
    if (coverage) {
      const coverageFactor = (coverage.lines / 100) * 2.5;
      score = Math.min(score, score * (coverageFactor / 2.5));
      factors.push({ factor: 'Coverage', weight: 0.25, score: coverageFactor });
    }

    // Performance factor (20% weight)
    if (lighthouse) {
      const perfFactor = (lighthouse.summary.avgPerformance / 100) * 2.0;
      score = Math.min(score, score * (perfFactor / 2.0));
      factors.push({ factor: 'Performance', weight: 0.2, score: perfFactor });
    }

    // Security factor (15% weight)
    if (security) {
      const securityPenalty =
        security.summary.critical * 0.5 + security.summary.high * 0.2;
      const securityFactor = Math.max(0, 1.5 - securityPenalty);
      score = Math.min(score, score * (securityFactor / 1.5));
      factors.push({ factor: 'Security', weight: 0.15, score: securityFactor });
    }

    return {
      overall: Math.max(0, Math.min(10, score)),
      factors,
      passed: score >= this.thresholds.quality.minScore,
    };
  }

  /**
   * Generate comprehensive test report
   */
  async generateReport() {
    console.log('📊 Generating comprehensive test report...');

    // Gather all test results
    const testResults = await this.processJestResults();
    const lighthouseResults = await this.processLighthouseResults();
    const securityResults = await this.processSecurityResults();

    // Calculate quality score
    const qualityScore = this.calculateQualityScore(
      testResults,
      testResults?.coverage,
      lighthouseResults,
      securityResults
    );

    // Generate final report
    const report = {
      timestamp: new Date().toISOString(),
      summary: {
        overallPassed: this.determineOverallStatus(
          testResults,
          lighthouseResults,
          securityResults,
          qualityScore
        ),
        qualityScore: qualityScore.overall,
        qualityPassed: qualityScore.passed,
      },
      results: {
        tests: testResults,
        lighthouse: lighthouseResults,
        security: securityResults,
      },
      qualityAnalysis: qualityScore,
      recommendations: this.generateRecommendations(
        testResults,
        lighthouseResults,
        securityResults
      ),
      metadata: {
        node: process.version,
        platform: process.platform,
        ci: process.env.CI === 'true',
        branch: process.env.GITHUB_REF || process.env.GIT_BRANCH || 'unknown',
        commit: process.env.GITHUB_SHA || process.env.GIT_COMMIT || 'unknown',
      },
    };

    // Save report
    await fs.writeFile(this.outputPath, JSON.stringify(report, null, 2));

    // Generate summary console output
    this.printSummary(report);

    return report;
  }

  /**
   * Determine overall CI/CD status
   */
  determineOverallStatus(tests, lighthouse, security, quality) {
    const factors = [];

    // Tests must pass
    if (tests && !tests.summary.success) {
      factors.push('Tests failed');
    }

    // Critical security issues block release
    if (security && security.summary.critical > 0) {
      factors.push('Critical security vulnerabilities');
    }

    // Performance regression blocks release
    if (
      lighthouse &&
      lighthouse.summary.passedPages < lighthouse.summary.totalPages
    ) {
      factors.push('Performance regression detected');
    }

    // Quality gate
    if (!quality.passed) {
      factors.push(
        `Quality score below threshold (${quality.overall.toFixed(1)} < ${this.thresholds.quality.minScore})`
      );
    }

    return {
      passed: factors.length === 0,
      blockingFactors: factors,
    };
  }

  /**
   * Generate actionable recommendations
   */
  generateRecommendations(tests, lighthouse, security) {
    const recommendations = [];

    // Test recommendations
    if (tests && tests.summary.failedTests > 0) {
      recommendations.push({
        category: 'Testing',
        priority: 'high',
        issue: `${tests.summary.failedTests} test(s) failing`,
        action: 'Fix failing tests before proceeding with deployment',
      });
    }

    // Coverage recommendations
    if (tests?.coverage && tests.coverage.lines < 80) {
      recommendations.push({
        category: 'Coverage',
        priority: 'medium',
        issue: `Low code coverage: ${tests.coverage.lines}%`,
        action: 'Add tests to improve coverage, especially for critical paths',
      });
    }

    // Performance recommendations
    if (lighthouse?.violations?.length > 0) {
      const perfViolations = lighthouse.violations.filter(
        v => v.type === 'performance'
      );
      if (perfViolations.length > 0) {
        recommendations.push({
          category: 'Performance',
          priority: 'high',
          issue: `${perfViolations.length} page(s) below performance threshold`,
          action:
            'Optimize JavaScript bundles, images, and server response times',
        });
      }
    }

    // Security recommendations
    if (security?.summary.critical > 0) {
      recommendations.push({
        category: 'Security',
        priority: 'critical',
        issue: `${security.summary.critical} critical security vulnerabilities`,
        action: 'Address all critical vulnerabilities before deployment',
      });
    }

    return recommendations;
  }

  /**
   * Print summary to console
   */
  printSummary(report) {
    console.log('\n🎯 CI/CD Quality Gate Summary');
    console.log('=' * 50);

    const status = report.summary.overallPassed.passed
      ? '✅ PASSED'
      : '❌ FAILED';
    console.log(`Overall Status: ${status}`);
    console.log(
      `Quality Score: ${report.summary.qualityScore.toFixed(1)}/10.0`
    );

    if (report.results.tests) {
      console.log(
        `\n📋 Tests: ${report.results.tests.summary.passedTests}/${report.results.tests.summary.totalTests} passed`
      );
      if (report.results.tests.coverage) {
        console.log(
          `🎯 Coverage: ${report.results.tests.coverage.lines}% lines`
        );
      }
    }

    if (report.results.lighthouse) {
      console.log(
        `⚡ Performance: ${report.results.lighthouse.summary.passedPages}/${report.results.lighthouse.summary.totalPages} pages passed`
      );
      console.log(
        `📊 Avg Performance Score: ${report.results.lighthouse.summary.avgPerformance}%`
      );
    }

    if (report.results.security) {
      console.log(
        `🔒 Security: ${report.results.security.summary.critical} critical, ${report.results.security.summary.high} high vulnerabilities`
      );
    }

    if (!report.summary.overallPassed.passed) {
      console.log('\n❌ Blocking Issues:');
      report.summary.overallPassed.blockingFactors.forEach(factor => {
        console.log(`  • ${factor}`);
      });
    }

    if (report.recommendations.length > 0) {
      console.log('\n💡 Recommendations:');
      report.recommendations.slice(0, 3).forEach(rec => {
        console.log(
          `  • [${rec.priority.toUpperCase()}] ${rec.issue}: ${rec.action}`
        );
      });
    }

    console.log(`\n📄 Full report: ${this.outputPath}`);
  }
}

module.exports = { TestResultsProcessor };

// CLI execution
if (require.main === module) {
  const processor = new TestResultsProcessor();

  processor
    .generateReport()
    .then(report => {
      if (!report.summary.overallPassed.passed) {
        console.error('\n❌ Quality gate failed!');
        process.exit(1);
      } else {
        console.log('\n✅ Quality gate passed!');
      }
    })
    .catch(error => {
      console.error('Report generation failed:', error);
      process.exit(1);
    });
}
