/**
 * Coverage Helpers for CI/CD Quality Gates
 * Provides utilities for coverage analysis, reporting, and enforcement
 */

const fs = require('fs').promises;
const path = require('path');

class CoverageEnforcer {
  constructor(options = {}) {
    this.thresholds = options.thresholds || {
      global: { branches: 80, functions: 80, lines: 80, statements: 80 },
      critical: { branches: 90, functions: 90, lines: 90, statements: 90 },
    };
    this.coverageDir =
      options.coverageDir || path.join(process.cwd(), 'tests/results/coverage');
    this.reportPath = path.join(this.coverageDir, 'coverage-summary.json');
    this.previousReportPath =
      options.previousReportPath ||
      path.join(this.coverageDir, 'coverage-previous.json');
  }

  /**
   * Read current coverage summary
   */
  async readCoverageSummary() {
    try {
      const data = await fs.readFile(this.reportPath, 'utf8');
      return JSON.parse(data);
    } catch (error) {
      console.warn('No coverage summary found:', error.message);
      return null;
    }
  }

  /**
   * Read previous coverage summary for comparison
   */
  async readPreviousCoverage() {
    try {
      const data = await fs.readFile(this.previousReportPath, 'utf8');
      return JSON.parse(data);
    } catch (error) {
      console.warn('No previous coverage found:', error.message);
      return null;
    }
  }

  /**
   * Save current coverage as previous for next run
   */
  async savePreviousCoverage(coverage) {
    try {
      await fs.writeFile(
        this.previousReportPath,
        JSON.stringify(coverage, null, 2)
      );
    } catch (error) {
      console.error('Failed to save previous coverage:', error.message);
    }
  }

  /**
   * Check if coverage meets thresholds
   */
  checkThresholds(coverage, thresholds = this.thresholds.global) {
    const failures = [];
    const metrics = ['branches', 'functions', 'lines', 'statements'];

    metrics.forEach(metric => {
      const actual = coverage.total[metric].pct;
      const required = thresholds[metric];

      if (actual < required) {
        failures.push({
          metric,
          actual,
          required,
          shortfall: required - actual,
        });
      }
    });

    return {
      passed: failures.length === 0,
      failures,
    };
  }

  /**
   * Compare coverage with previous run
   */
  compareCoverage(current, previous) {
    if (!previous) {
      return {
        hasRegression: false,
        message: 'No previous coverage to compare',
      };
    }

    const regressions = [];
    const improvements = [];
    const metrics = ['branches', 'functions', 'lines', 'statements'];

    metrics.forEach(metric => {
      const currentPct = current.total[metric].pct;
      const previousPct = previous.total[metric].pct;
      const diff = currentPct - previousPct;

      if (diff < -1) {
        // Regression if drop > 1%
        regressions.push({
          metric,
          current: currentPct,
          previous: previousPct,
          regression: Math.abs(diff),
        });
      } else if (diff > 1) {
        // Improvement if gain > 1%
        improvements.push({
          metric,
          current: currentPct,
          previous: previousPct,
          improvement: diff,
        });
      }
    });

    return {
      hasRegression: regressions.length > 0,
      regressions,
      improvements,
      message: this.generateComparisonMessage(regressions, improvements),
    };
  }

  /**
   * Generate human-readable comparison message
   */
  generateComparisonMessage(regressions, improvements) {
    const messages = [];

    if (regressions.length > 0) {
      messages.push(`❌ Coverage regressions detected:`);
      regressions.forEach(reg => {
        messages.push(
          `  • ${reg.metric}: ${reg.current}% (was ${reg.previous}%, -${reg.regression.toFixed(1)}%)`
        );
      });
    }

    if (improvements.length > 0) {
      messages.push(`✅ Coverage improvements:`);
      improvements.forEach(imp => {
        messages.push(
          `  • ${imp.metric}: ${imp.current}% (was ${imp.previous}%, +${imp.improvement.toFixed(1)}%)`
        );
      });
    }

    return messages.join('\n');
  }

  /**
   * Generate coverage report for CI/CD
   */
  async generateCoverageReport() {
    const coverage = await this.readCoverageSummary();
    const previous = await this.readPreviousCoverage();

    if (!coverage) {
      return {
        passed: false,
        error: 'No coverage data available',
      };
    }

    const thresholdCheck = this.checkThresholds(coverage);
    const comparison = this.compareCoverage(coverage, previous);

    const report = {
      timestamp: new Date().toISOString(),
      passed: thresholdCheck.passed && !comparison.hasRegression,
      coverage: {
        branches: coverage.total.branches.pct,
        functions: coverage.total.functions.pct,
        lines: coverage.total.lines.pct,
        statements: coverage.total.statements.pct,
      },
      thresholds: this.thresholds.global,
      thresholdCheck,
      comparison,
      files: this.analyzeFileCoverage(coverage),
    };

    // Save current coverage for next comparison
    await this.savePreviousCoverage(coverage);

    // Save the report
    const reportOutputPath = path.join(
      this.coverageDir,
      'coverage-report.json'
    );
    await fs.writeFile(reportOutputPath, JSON.stringify(report, null, 2));

    return report;
  }

  /**
   * Analyze individual file coverage
   */
  analyzeFileCoverage(coverage) {
    const files = Object.entries(coverage)
      .filter(([key]) => key !== 'total')
      .map(([filePath, fileCoverage]) => ({
        path: filePath,
        branches: fileCoverage.branches.pct,
        functions: fileCoverage.functions.pct,
        lines: fileCoverage.lines.pct,
        statements: fileCoverage.statements.pct,
        uncoveredLines: this.getUncoveredLines(fileCoverage),
      }))
      .sort((a, b) => {
        // Sort by lowest line coverage first (most problematic files)
        return a.lines - b.lines;
      });

    return {
      total: files.length,
      belowThreshold: files.filter(
        f =>
          f.lines < this.thresholds.global.lines ||
          f.branches < this.thresholds.global.branches
      ),
      needsAttention: files.slice(0, 10), // Top 10 files needing attention
    };
  }

  /**
   * Extract uncovered lines for file
   */
  getUncoveredLines(fileCoverage) {
    if (!fileCoverage.s) return [];

    return Object.entries(fileCoverage.s)
      .filter(([, count]) => count === 0)
      .map(([line]) => parseInt(line, 10))
      .sort((a, b) => a - b);
  }

  /**
   * Generate coverage badge data
   */
  generateBadgeData(coverage) {
    const overallPct = Math.round(coverage.total.lines.pct);
    let color;

    if (overallPct >= 90) color = 'brightgreen';
    else if (overallPct >= 80) color = 'green';
    else if (overallPct >= 70) color = 'yellow';
    else if (overallPct >= 60) color = 'orange';
    else color = 'red';

    return {
      schemaVersion: 1,
      label: 'coverage',
      message: `${overallPct}%`,
      color,
    };
  }

  /**
   * Main enforcement method for CI/CD
   */
  async enforceCoverage() {
    console.log('🔍 Analyzing code coverage...');

    const report = await this.generateCoverageReport();

    if (report.error) {
      console.error('❌ Coverage analysis failed:', report.error);
      process.exit(1);
    }

    // Print coverage summary
    console.log('\n📊 Coverage Summary:');
    console.log(`  Branches: ${report.coverage.branches}%`);
    console.log(`  Functions: ${report.coverage.functions}%`);
    console.log(`  Lines: ${report.coverage.lines}%`);
    console.log(`  Statements: ${report.coverage.statements}%`);

    // Print threshold check results
    if (!report.thresholdCheck.passed) {
      console.log('\n❌ Coverage thresholds not met:');
      report.thresholdCheck.failures.forEach(failure => {
        console.log(
          `  • ${failure.metric}: ${failure.actual}% (required: ${failure.required}%, shortfall: ${failure.shortfall.toFixed(1)}%)`
        );
      });
    } else {
      console.log('\n✅ All coverage thresholds met');
    }

    // Print comparison results
    if (report.comparison.message) {
      console.log('\n📈 Coverage Comparison:');
      console.log(report.comparison.message);
    }

    // Print files needing attention
    if (report.files.needsAttention.length > 0) {
      console.log('\n🎯 Files needing coverage attention:');
      report.files.needsAttention.slice(0, 5).forEach(file => {
        console.log(`  • ${file.path}: ${file.lines}% lines`);
      });
    }

    if (!report.passed) {
      console.log('\n❌ Coverage quality gate failed!');
      process.exit(1);
    } else {
      console.log('\n✅ Coverage quality gate passed!');
    }

    return report;
  }
}

// Export for use in Jest and CI scripts
module.exports = {
  CoverageEnforcer,

  // Jest setup helper
  setupCoverageEnforcement: () => {
    const enforcer = new CoverageEnforcer();

    // Add global teardown for coverage enforcement
    if (process.env.COVERAGE === 'true') {
      process.on('exit', () => {
        // Note: In actual CI, this would be handled by a separate script
        // since Jest's exit handlers have limitations
      });
    }

    return enforcer;
  },
};

// CLI execution
if (require.main === module) {
  const enforcer = new CoverageEnforcer();
  enforcer.enforceCoverage().catch(error => {
    console.error('Coverage enforcement failed:', error);
    process.exit(1);
  });
}
