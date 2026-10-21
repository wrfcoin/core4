/**
 * Continuous Fuzzing Orchestrator
 * Coordinates all fuzzing activities and provides centralized control
 */

const RestApiFuzzer = require('./rest-api-fuzzer');
const WebSocketFuzzer = require('./websocket-fuzzer');
const { spawn } = require('child_process');
const winston = require('winston');
const config = require('../config/fuzzing-config');
const FuzzReporter = require('../reporting/fuzz-reporter');

class ContinuousFuzzer {
  constructor(options = {}) {
    this.config = { ...config, ...options };
    this.reporter = new FuzzReporter('continuous');
    this.setupLogger();
    this.isRunning = false;
    this.results = {
      rest: [],
      websocket: [],
      property: [],
      startTime: null,
      endTime: null,
    };
  }

  setupLogger() {
    this.logger = winston.createLogger({
      level: 'info',
      format: winston.format.combine(
        winston.format.timestamp(),
        winston.format.json()
      ),
      transports: [
        new winston.transports.Console(),
        new winston.transports.File({
          filename: 'fuzz-testing/logs/continuous-fuzzer.log',
        }),
      ],
    });
  }

  /**
   * Start continuous fuzzing with specified duration
   */
  async startFuzzing(duration = this.config.fuzzing.duration.standard) {
    if (this.isRunning) {
      throw new Error('Fuzzing is already running');
    }

    this.isRunning = true;
    this.results.startTime = Date.now();

    this.logger.info('Starting continuous fuzzing session', {
      duration: duration === -1 ? 'indefinite' : `${duration}ms`,
      timestamp: new Date().toISOString(),
    });

    try {
      // Run all fuzzing types in parallel
      const fuzzingPromises = [];

      // REST API Fuzzing
      if (this.shouldRunRestFuzzing()) {
        fuzzingPromises.push(this.runRestApiFuzzing(duration));
      }

      // WebSocket Fuzzing
      if (this.shouldRunWebSocketFuzzing()) {
        fuzzingPromises.push(this.runWebSocketFuzzing(duration));
      }

      // Property-Based Testing
      if (this.shouldRunPropertyTesting()) {
        fuzzingPromises.push(this.runPropertyBasedTesting());
      }

      // Wait for all fuzzing to complete or timeout
      if (duration > 0) {
        await Promise.race([
          Promise.all(fuzzingPromises),
          this.createTimeoutPromise(duration),
        ]);
      } else {
        await Promise.all(fuzzingPromises);
      }
    } catch (error) {
      this.logger.error('Fuzzing session failed', error);
      throw error;
    } finally {
      this.isRunning = false;
      this.results.endTime = Date.now();
    }

    // Generate comprehensive report
    const report = await this.generateFinalReport();

    this.logger.info('Continuous fuzzing session completed', {
      duration: this.results.endTime - this.results.startTime,
      vulnerabilities: this.getTotalVulnerabilities(),
    });

    return report;
  }

  async runRestApiFuzzing(duration) {
    try {
      this.logger.info('Starting REST API fuzzing');
      const restFuzzer = new RestApiFuzzer();
      const restDuration = duration > 0 ? Math.floor(duration * 0.4) : -1; // 40% of total time
      const result = await restFuzzer.runComprehensiveFuzzing(restDuration);
      this.results.rest.push(result);
      this.logger.info('REST API fuzzing completed', {
        vulnerabilities: result.summary.totalVulnerabilities,
      });
    } catch (error) {
      this.logger.error('REST API fuzzing failed', error);
      this.results.rest.push({
        error: error.message,
        timestamp: new Date().toISOString(),
      });
    }
  }

  async runWebSocketFuzzing(duration) {
    try {
      this.logger.info('Starting WebSocket fuzzing');
      const wsFuzzer = new WebSocketFuzzer();
      const wsDuration = duration > 0 ? Math.floor(duration * 0.3) : -1; // 30% of total time
      const result = await wsFuzzer.runComprehensiveFuzzing(wsDuration);
      this.results.websocket.push(result);
      this.logger.info('WebSocket fuzzing completed', {
        vulnerabilities: result.summary.totalVulnerabilities,
      });
    } catch (error) {
      this.logger.error('WebSocket fuzzing failed', error);
      this.results.websocket.push({
        error: error.message,
        timestamp: new Date().toISOString(),
      });
    }
  }

  async runPropertyBasedTesting() {
    try {
      this.logger.info('Starting property-based testing');
      const result = await this.executePropertyTests();
      this.results.property.push(result);
      this.logger.info('Property-based testing completed', {
        testsRun: result.numPassed + result.numFailed,
        failures: result.numFailed,
      });
    } catch (error) {
      this.logger.error('Property-based testing failed', error);
      this.results.property.push({
        error: error.message,
        timestamp: new Date().toISOString(),
      });
    }
  }

  executePropertyTests() {
    return new Promise((resolve, reject) => {
      const jestProcess = spawn(
        'npx',
        ['jest', '--config=fuzzing/property-based/jest.config.js', '--json'],
        {
          cwd: process.cwd(),
          stdio: ['pipe', 'pipe', 'pipe'],
        }
      );

      let stdout = '';
      let stderr = '';

      jestProcess.stdout.on('data', data => {
        stdout += data.toString();
      });

      jestProcess.stderr.on('data', data => {
        stderr += data.toString();
      });

      jestProcess.on('close', code => {
        try {
          const result = JSON.parse(stdout);
          resolve({
            timestamp: new Date().toISOString(),
            success: code === 0,
            numPassedTests: result.numPassedTests,
            numFailedTests: result.numFailedTests,
            numTotalTests: result.numTotalTests,
            testResults: result.testResults,
            coverageMap: result.coverageMap,
          });
        } catch (parseError) {
          reject(
            new Error(
              `Failed to parse Jest output: ${parseError.message}\nStdout: ${stdout}\nStderr: ${stderr}`
            )
          );
        }
      });

      jestProcess.on('error', error => {
        reject(new Error(`Jest process failed: ${error.message}`));
      });
    });
  }

  shouldRunRestFuzzing() {
    // Check if any API servers are configured and accessible
    return Object.keys(this.config.apiServers).length > 0;
  }

  shouldRunWebSocketFuzzing() {
    // Check if WebSocket URL is configured
    return this.config.websocket && this.config.websocket.url;
  }

  shouldRunPropertyTesting() {
    // Always run property tests as they don't require external services
    return true;
  }

  createTimeoutPromise(duration) {
    return new Promise(resolve => {
      setTimeout(() => {
        this.logger.info('Fuzzing duration reached, stopping all tests');
        resolve();
      }, duration);
    });
  }

  async generateFinalReport() {
    const totalDuration = this.results.endTime - this.results.startTime;

    const consolidatedReport = {
      summary: {
        timestamp: new Date().toISOString(),
        totalDuration,
        testTypes: {
          rest: this.results.rest.length,
          websocket: this.results.websocket.length,
          property: this.results.property.length,
        },
        totalVulnerabilities: this.getTotalVulnerabilities(),
        totalErrors: this.getTotalErrors(),
        overallRiskScore: this.calculateOverallRiskScore(),
      },
      restApiResults: this.results.rest,
      websocketResults: this.results.websocket,
      propertyTestResults: this.results.property,
      vulnerabilities: this.getAllVulnerabilities(),
      recommendations: this.generateConsolidatedRecommendations(),
      executiveSummary: this.generateExecutiveSummary(),
    };

    // Save the comprehensive report
    await this.reporter.saveReport(
      consolidatedReport,
      'continuous-fuzz-comprehensive'
    );

    return consolidatedReport;
  }

  getTotalVulnerabilities() {
    let total = 0;

    this.results.rest.forEach(result => {
      total += result.summary?.totalVulnerabilities || 0;
    });

    this.results.websocket.forEach(result => {
      total += result.summary?.totalVulnerabilities || 0;
    });

    return total;
  }

  getTotalErrors() {
    let total = 0;

    this.results.rest.forEach(result => {
      total += result.summary?.totalErrors || 0;
    });

    this.results.websocket.forEach(result => {
      total += result.summary?.totalErrors || 0;
    });

    this.results.property.forEach(result => {
      total += result.numFailedTests || 0;
    });

    return total;
  }

  getAllVulnerabilities() {
    const vulnerabilities = [];

    this.results.rest.forEach(result => {
      if (result.vulnerabilities) {
        vulnerabilities.push(
          ...result.vulnerabilities.map(v => ({ ...v, source: 'rest-api' }))
        );
      }
    });

    this.results.websocket.forEach(result => {
      if (result.vulnerabilities) {
        vulnerabilities.push(
          ...result.vulnerabilities.map(v => ({ ...v, source: 'websocket' }))
        );
      }
    });

    return vulnerabilities;
  }

  calculateOverallRiskScore() {
    const vulnerabilities = this.getAllVulnerabilities();
    const weights = { critical: 10, high: 7, medium: 4, low: 1 };

    return vulnerabilities.reduce((score, vuln) => {
      const severity = vuln.severity || 'medium';
      return score + (weights[severity] || weights.medium);
    }, 0);
  }

  generateConsolidatedRecommendations() {
    const recommendations = [];

    // Collect recommendations from all sources
    this.results.rest.forEach(result => {
      if (result.recommendations) {
        recommendations.push(
          ...result.recommendations.map(r => ({ ...r, source: 'rest-api' }))
        );
      }
    });

    this.results.websocket.forEach(result => {
      if (result.recommendations) {
        recommendations.push(
          ...result.recommendations.map(r => ({ ...r, source: 'websocket' }))
        );
      }
    });

    // Add general recommendations based on overall findings
    const vulnerabilities = this.getAllVulnerabilities();
    const totalVulns = vulnerabilities.length;
    const highSeverityVulns = vulnerabilities.filter(
      v => v.severity === 'critical' || v.severity === 'high'
    ).length;

    if (totalVulns > 10) {
      recommendations.push({
        type: 'general',
        priority: 'HIGH',
        message: `High number of vulnerabilities found (${totalVulns}). Consider implementing a comprehensive security review process.`,
        source: 'analysis',
      });
    }

    if (highSeverityVulns > 0) {
      recommendations.push({
        type: 'security',
        priority: 'CRITICAL',
        message: `${highSeverityVulns} high-severity vulnerabilities require immediate attention.`,
        source: 'analysis',
      });
    }

    return recommendations;
  }

  generateExecutiveSummary() {
    const totalVulns = this.getTotalVulnerabilities();
    const totalErrors = this.getTotalErrors();
    const riskScore = this.calculateOverallRiskScore();

    let riskLevel = 'LOW';
    if (riskScore > 30) riskLevel = 'CRITICAL';
    else if (riskScore > 15) riskLevel = 'HIGH';
    else if (riskScore > 5) riskLevel = 'MEDIUM';

    return {
      overallStatus: totalVulns === 0 ? 'PASS' : 'FAIL',
      riskLevel,
      keyFindings: [
        `${totalVulns} security vulnerabilities identified across all testing methods`,
        `${totalErrors} errors encountered during testing`,
        `Risk score: ${riskScore} (${riskLevel})`,
      ],
      nextSteps: this.generateNextSteps(totalVulns, riskLevel),
      complianceImpact: {
        status: totalVulns > 0 ? 'NON_COMPLIANT' : 'COMPLIANT',
        frameworks: ['PCI-DSS', 'ISO-27001', 'SOX'],
        notes:
          totalVulns > 0
            ? 'Security vulnerabilities may impact compliance status'
            : 'No security issues detected',
      },
    };
  }

  generateNextSteps(vulnerabilityCount, riskLevel) {
    const steps = [];

    if (vulnerabilityCount > 0) {
      steps.push(
        'Prioritize fixing high and critical severity vulnerabilities'
      );
      steps.push('Review and update input validation across all endpoints');
      steps.push('Implement comprehensive security testing in CI/CD pipeline');
    }

    if (riskLevel === 'CRITICAL' || riskLevel === 'HIGH') {
      steps.push('Conduct immediate security audit with external consultants');
      steps.push(
        'Consider temporarily restricting access to affected endpoints'
      );
    }

    steps.push(
      'Schedule regular fuzzing tests as part of security maintenance'
    );
    steps.push(
      'Update security documentation and incident response procedures'
    );

    return steps;
  }

  /**
   * Stop continuous fuzzing (for long-running sessions)
   */
  async stopFuzzing() {
    if (!this.isRunning) {
      this.logger.warn('No fuzzing session is currently running');
      return;
    }

    this.logger.info('Stopping continuous fuzzing session');
    this.isRunning = false;

    // Generate report with current results
    const report = await this.generateFinalReport();

    this.logger.info('Fuzzing session stopped and report generated');
    return report;
  }

  /**
   * Get current status of fuzzing session
   */
  getStatus() {
    return {
      isRunning: this.isRunning,
      startTime: this.results.startTime,
      currentTime: Date.now(),
      vulnerabilitiesFound: this.getTotalVulnerabilities(),
      errorsEncountered: this.getTotalErrors(),
      testsCompleted: {
        rest: this.results.rest.length,
        websocket: this.results.websocket.length,
        property: this.results.property.length,
      },
    };
  }
}

module.exports = ContinuousFuzzer;

// Command line interface
if (require.main === module) {
  const args = process.argv.slice(2);
  const duration = args.includes('--duration')
    ? parseInt(args[args.indexOf('--duration') + 1]) * 1000
    : 300000; // 5 minutes default

  const fuzzer = new ContinuousFuzzer();

  // Handle graceful shutdown
  process.on('SIGINT', async () => {
    console.log('\nReceived SIGINT, stopping fuzzing...');
    try {
      await fuzzer.stopFuzzing();
      process.exit(0);
    } catch (error) {
      console.error('Error during shutdown:', error);
      process.exit(1);
    }
  });

  fuzzer
    .startFuzzing(duration)
    .then(report => {
      console.log('\n🎉 Comprehensive fuzzing completed!');
      console.log(
        `📊 Total vulnerabilities found: ${report.summary.totalVulnerabilities}`
      );
      console.log(
        `🚨 Overall risk level: ${report.executiveSummary.riskLevel}`
      );
      console.log(`📋 Report saved with comprehensive details`);
      process.exit(0);
    })
    .catch(error => {
      console.error('\n❌ Fuzzing failed:', error.message);
      process.exit(1);
    });
}
