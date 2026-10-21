/**
 * Integration Test Orchestrator
 * Coordinates automated execution of integration test suites with mock services
 */

const { execSync, spawn } = require('child_process');
const fs = require('fs');
const path = require('path');
const { promisify } = require('util');

const exec = promisify(require('child_process').exec);

class IntegrationTestOrchestrator {
  constructor(config = {}) {
    this.config = {
      // Test execution configuration
      maxParallelWorkers: config.maxParallelWorkers || 4,
      testTimeout: config.testTimeout || 90000,
      retryAttempts: config.retryAttempts || 2,

      // Mock service configuration
      mockServicesEnabled: config.mockServicesEnabled ?? true,
      waitForServicesTimeout: config.waitForServicesTimeout || 120000,

      // Reporting configuration
      generateReports: config.generateReports ?? true,
      reportFormats: config.reportFormats || ['html', 'json', 'junit'],

      // Docker configuration
      dockerComposeFile:
        config.dockerComposeFile || 'tests/docker-compose.test.yml',
      dockerComposeProfile: config.dockerComposeProfile || 'testing',

      // Service URLs
      services: {
        blockchain: config.blockchainUrl || 'http://localhost:8545',
        weather: config.weatherUrl || 'http://localhost:3010',
        carbon: config.carbonUrl || 'http://localhost:3020',
        mockserver: config.mockserverUrl || 'http://localhost:1080',
        postgres:
          config.postgresUrl ||
          'postgresql://postgres:testpass@localhost:5432/wrfcoin_test',
        redis: config.redisUrl || 'redis://localhost:6379',
      },

      ...config,
    };

    this.results = {
      startTime: null,
      endTime: null,
      totalTests: 0,
      passedTests: 0,
      failedTests: 0,
      skippedTests: 0,
      suiteResults: [],
      serviceHealthChecks: {},
      performanceMetrics: {},
      errors: [],
    };

    this.testSuites = [
      {
        name: 'Service Interaction Tests',
        path: 'tests/integration-automation/suites/service-interaction.test.js',
        priority: 1,
        timeout: 60000,
      },
      {
        name: 'API Integration Tests',
        path: 'tests/integration-automation/suites/api-integration.test.js',
        priority: 2,
        timeout: 90000,
      },
      {
        name: 'End-to-End Integration Tests',
        path: 'tests/integration-automation/suites/end-to-end.test.js',
        priority: 3,
        timeout: 120000,
      },
      {
        name: 'Existing Integration Tests',
        path: 'tests/integration/api-integration.test.js',
        priority: 4,
        timeout: 90000,
      },
    ];
  }

  /**
   * Main orchestration method - runs complete integration test automation
   */
  async orchestrateTests() {
    console.log('🚀 Starting Integration Test Orchestration...');
    this.results.startTime = new Date();

    try {
      // Phase 1: Pre-execution setup
      await this.setupTestEnvironment();

      // Phase 2: Start mock services
      if (this.config.mockServicesEnabled) {
        await this.startMockServices();
        await this.waitForServicesReady();
        await this.validateServiceHealth();
      }

      // Phase 3: Execute test suites in priority order
      await this.executeTestSuites();

      // Phase 4: Performance monitoring and metrics collection
      await this.collectPerformanceMetrics();

      // Phase 5: Generate comprehensive reports
      if (this.config.generateReports) {
        await this.generateReports();
      }

      // Phase 6: Cleanup
      await this.cleanup();

      this.results.endTime = new Date();
      return this.generateSummary();
    } catch (error) {
      this.results.errors.push({
        timestamp: new Date(),
        phase: 'orchestration',
        error: error.message,
        stack: error.stack,
      });

      await this.cleanup();
      throw error;
    }
  }

  /**
   * Setup test environment and validate prerequisites
   */
  async setupTestEnvironment() {
    console.log('🔧 Setting up test environment...');

    // Create results directory
    const resultsDir = path.join(process.cwd(), 'tests/results');
    if (!fs.existsSync(resultsDir)) {
      fs.mkdirSync(resultsDir, { recursive: true });
    }

    // Create automation-specific directories
    const automationResultsDir = path.join(
      resultsDir,
      'integration-automation'
    );
    if (!fs.existsSync(automationResultsDir)) {
      fs.mkdirSync(automationResultsDir, { recursive: true });
    }

    // Validate Docker availability
    try {
      await exec('docker --version');
      await exec('docker-compose --version');
    } catch (error) {
      throw new Error(
        'Docker or Docker Compose not available. Integration tests require Docker.'
      );
    }

    // Validate test files exist
    for (const suite of this.testSuites) {
      if (!fs.existsSync(suite.path)) {
        console.warn(`⚠️  Test suite not found: ${suite.path}`);
      }
    }

    console.log('✅ Test environment setup complete');
  }

  /**
   * Start mock services using Docker Compose
   */
  async startMockServices() {
    console.log('🏗️  Starting mock services...');

    try {
      const dockerComposeCmd = `docker-compose -f ${this.config.dockerComposeFile} --profile ${this.config.dockerComposeProfile} up -d`;
      console.log(`Executing: ${dockerComposeCmd}`);

      const { stdout, stderr } = await exec(dockerComposeCmd, {
        timeout: 120000,
      });

      if (
        stderr &&
        !stderr.includes('Creating') &&
        !stderr.includes('Starting')
      ) {
        console.warn('Docker Compose stderr:', stderr);
      }

      console.log('✅ Mock services started successfully');
    } catch (error) {
      console.error('❌ Failed to start mock services:', error.message);
      throw new Error(`Mock service startup failed: ${error.message}`);
    }
  }

  /**
   * Wait for all services to be ready
   */
  async waitForServicesReady() {
    console.log('⏳ Waiting for services to be ready...');

    const startTime = Date.now();
    const timeout = this.config.waitForServicesTimeout;

    while (Date.now() - startTime < timeout) {
      try {
        const healthChecks = await Promise.allSettled([
          this.checkServiceHealth(
            'blockchain',
            `${this.config.services.blockchain}/health`
          ),
          this.checkServiceHealth(
            'weather',
            `${this.config.services.weather}/health`
          ),
          this.checkServiceHealth(
            'carbon',
            `${this.config.services.carbon}/health`
          ),
        ]);

        const healthyServices = healthChecks.filter(
          check => check.status === 'fulfilled'
        ).length;

        if (healthyServices === healthChecks.length) {
          console.log('✅ All services are ready');
          return;
        }

        console.log(
          `⏳ ${healthyServices}/${healthChecks.length} services ready, waiting...`
        );
        await new Promise(resolve => setTimeout(resolve, 5000));
      } catch (error) {
        console.log('⏳ Services not ready yet, continuing to wait...');
        await new Promise(resolve => setTimeout(resolve, 5000));
      }
    }

    throw new Error(`Services failed to become ready within ${timeout}ms`);
  }

  /**
   * Check individual service health
   */
  async checkServiceHealth(serviceName, healthUrl) {
    const fetch = (await import('node-fetch')).default;
    const response = await fetch(healthUrl, {
      timeout: 5000,
      headers: { Accept: 'application/json' },
    });

    if (!response.ok) {
      throw new Error(`${serviceName} health check failed: ${response.status}`);
    }

    const healthData = await response.json();
    return { serviceName, status: 'healthy', data: healthData };
  }

  /**
   * Validate service health and collect baseline metrics
   */
  async validateServiceHealth() {
    console.log('🩺 Validating service health...');

    const healthPromises = Object.entries(this.config.services)
      .filter(([name]) => ['blockchain', 'weather', 'carbon'].includes(name))
      .map(async ([serviceName, serviceUrl]) => {
        try {
          const healthUrl = `${serviceUrl}/health`;
          const result = await this.checkServiceHealth(serviceName, healthUrl);
          this.results.serviceHealthChecks[serviceName] = result;
          return result;
        } catch (error) {
          this.results.serviceHealthChecks[serviceName] = {
            serviceName,
            status: 'unhealthy',
            error: error.message,
          };
          throw error;
        }
      });

    await Promise.all(healthPromises);
    console.log('✅ Service health validation complete');
  }

  /**
   * Execute test suites in priority order
   */
  async executeTestSuites() {
    console.log('🧪 Executing integration test suites...');

    // Sort by priority
    const sortedSuites = [...this.testSuites].sort(
      (a, b) => a.priority - b.priority
    );

    for (const suite of sortedSuites) {
      if (!fs.existsSync(suite.path)) {
        console.warn(`⚠️  Skipping missing test suite: ${suite.name}`);
        continue;
      }

      console.log(`🏃 Running: ${suite.name}`);

      try {
        const suiteResult = await this.executeSingleSuite(suite);
        this.results.suiteResults.push(suiteResult);

        this.results.totalTests += suiteResult.totalTests;
        this.results.passedTests += suiteResult.passedTests;
        this.results.failedTests += suiteResult.failedTests;
        this.results.skippedTests += suiteResult.skippedTests;

        console.log(
          `✅ ${suite.name}: ${suiteResult.passedTests}/${suiteResult.totalTests} passed`
        );
      } catch (error) {
        console.error(`❌ ${suite.name} failed:`, error.message);

        this.results.suiteResults.push({
          name: suite.name,
          status: 'failed',
          error: error.message,
          totalTests: 0,
          passedTests: 0,
          failedTests: 1,
          skippedTests: 0,
          duration: 0,
        });

        this.results.failedTests += 1;
        this.results.errors.push({
          timestamp: new Date(),
          phase: 'test-execution',
          suite: suite.name,
          error: error.message,
        });

        // Continue with other suites unless critical failure
        if (!this.config.continueOnFailure) {
          throw error;
        }
      }
    }

    console.log(
      `🏁 Test execution complete: ${this.results.passedTests}/${this.results.totalTests} passed`
    );
  }

  /**
   * Execute a single test suite
   */
  async executeSingleSuite(suite) {
    const startTime = Date.now();

    try {
      const jestCmd = `npx jest --config=tests/jest.integration.config.js --testPathPattern="${suite.path}" --reporters=default --reporters=json --outputFile=tests/results/suite-${suite.name.replace(/\s+/g, '-').toLowerCase()}.json --verbose`;

      console.log(`Executing: ${jestCmd}`);

      const { stdout, stderr } = await exec(jestCmd, {
        timeout: suite.timeout,
        maxBuffer: 1024 * 1024 * 10, // 10MB buffer
      });

      const duration = Date.now() - startTime;

      // Parse Jest output for results
      const results = this.parseJestOutput(stdout, stderr);

      return {
        name: suite.name,
        status: 'completed',
        duration,
        totalTests: results.totalTests,
        passedTests: results.passedTests,
        failedTests: results.failedTests,
        skippedTests: results.skippedTests,
        output: stdout,
        stderr: stderr,
      };
    } catch (error) {
      const duration = Date.now() - startTime;

      // Handle timeout specifically
      if (error.killed && error.signal === 'SIGTERM') {
        throw new Error(`Test suite timed out after ${suite.timeout}ms`);
      }

      throw new Error(`Test execution failed: ${error.message}`);
    }
  }

  /**
   * Parse Jest output to extract test results
   */
  parseJestOutput(stdout, stderr) {
    const results = {
      totalTests: 0,
      passedTests: 0,
      failedTests: 0,
      skippedTests: 0,
    };

    try {
      // Look for Jest summary pattern
      const summaryMatch = stdout.match(
        /Tests:\s+(\d+)\s+failed,\s+(\d+)\s+passed,\s+(\d+)\s+total/
      );
      if (summaryMatch) {
        results.failedTests = parseInt(summaryMatch[1]);
        results.passedTests = parseInt(summaryMatch[2]);
        results.totalTests = parseInt(summaryMatch[3]);
        return results;
      }

      // Alternative pattern
      const altMatch = stdout.match(/Tests:\s+(\d+)\s+passed,\s+(\d+)\s+total/);
      if (altMatch) {
        results.passedTests = parseInt(altMatch[1]);
        results.totalTests = parseInt(altMatch[2]);
        return results;
      }

      // Fallback: count test patterns
      const testMatches = stdout.match(/✓|✗|○/g);
      if (testMatches) {
        results.totalTests = testMatches.length;
        results.passedTests = (stdout.match(/✓/g) || []).length;
        results.failedTests = (stdout.match(/✗/g) || []).length;
        results.skippedTests = (stdout.match(/○/g) || []).length;
      }
    } catch (error) {
      console.warn('Failed to parse Jest output:', error.message);
    }

    return results;
  }

  /**
   * Collect performance metrics from services and test execution
   */
  async collectPerformanceMetrics() {
    console.log('📊 Collecting performance metrics...');

    const fetch = (await import('node-fetch')).default;

    try {
      // Collect service metrics
      const metricsPromises = Object.entries(this.config.services)
        .filter(([name]) => ['blockchain', 'weather', 'carbon'].includes(name))
        .map(async ([serviceName, serviceUrl]) => {
          try {
            const metricsUrl = `${serviceUrl}/metrics`;
            const response = await fetch(metricsUrl, { timeout: 5000 });

            if (response.ok) {
              const metrics = await response.json();
              this.results.performanceMetrics[serviceName] = metrics;
            }
          } catch (error) {
            console.warn(
              `Failed to collect metrics from ${serviceName}:`,
              error.message
            );
          }
        });

      await Promise.allSettled(metricsPromises);

      // Add orchestration metrics
      this.results.performanceMetrics.orchestration = {
        totalDuration: this.results.endTime
          ? this.results.endTime.getTime() - this.results.startTime.getTime()
          : Date.now() - this.results.startTime.getTime(),
        avgTestDuration:
          this.results.suiteResults.length > 0
            ? this.results.suiteResults.reduce(
                (sum, suite) => sum + suite.duration,
                0
              ) / this.results.suiteResults.length
            : 0,
        successRate:
          this.results.totalTests > 0
            ? (this.results.passedTests / this.results.totalTests) * 100
            : 0,
      };

      console.log('✅ Performance metrics collected');
    } catch (error) {
      console.warn('Failed to collect performance metrics:', error.message);
    }
  }

  /**
   * Generate comprehensive reports
   */
  async generateReports() {
    console.log('📋 Generating test reports...');

    const reportData = {
      timestamp: new Date().toISOString(),
      configuration: this.config,
      results: this.results,
      summary: this.generateSummary(),
    };

    const reportsDir = path.join(
      process.cwd(),
      'tests/results/integration-automation'
    );

    // Generate JSON report
    if (this.config.reportFormats.includes('json')) {
      const jsonReportPath = path.join(reportsDir, 'orchestration-report.json');
      fs.writeFileSync(jsonReportPath, JSON.stringify(reportData, null, 2));
      console.log(`📄 JSON report: ${jsonReportPath}`);
    }

    // Generate HTML report
    if (this.config.reportFormats.includes('html')) {
      const htmlReport = this.generateHtmlReport(reportData);
      const htmlReportPath = path.join(reportsDir, 'orchestration-report.html');
      fs.writeFileSync(htmlReportPath, htmlReport);
      console.log(`📄 HTML report: ${htmlReportPath}`);
    }

    // Generate JUnit XML report
    if (this.config.reportFormats.includes('junit')) {
      const junitReport = this.generateJunitReport(reportData);
      const junitReportPath = path.join(reportsDir, 'orchestration-junit.xml');
      fs.writeFileSync(junitReportPath, junitReport);
      console.log(`📄 JUnit report: ${junitReportPath}`);
    }

    console.log('✅ Reports generated successfully');
  }

  /**
   * Generate HTML report
   */
  generateHtmlReport(reportData) {
    return `
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>WRFCoin Integration Test Automation Report</title>
    <style>
        body { font-family: 'Segoe UI', Tahoma, Geneva, Verdana, sans-serif; margin: 0; padding: 20px; background: #f5f5f5; }
        .container { max-width: 1200px; margin: 0 auto; background: white; padding: 30px; border-radius: 8px; box-shadow: 0 2px 10px rgba(0,0,0,0.1); }
        .header { text-align: center; margin-bottom: 30px; }
        .summary { display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 20px; margin: 30px 0; }
        .metric { background: linear-gradient(135deg, #667eea 0%, #764ba2 100%); color: white; padding: 20px; border-radius: 8px; text-align: center; }
        .metric h3 { margin: 0 0 10px 0; font-size: 14px; opacity: 0.9; }
        .metric .value { font-size: 32px; font-weight: bold; }
        .suite { margin: 20px 0; padding: 20px; border: 1px solid #e0e0e0; border-radius: 8px; }
        .suite.passed { border-left: 4px solid #4caf50; }
        .suite.failed { border-left: 4px solid #f44336; }
        .performance { margin: 30px 0; }
        .table { width: 100%; border-collapse: collapse; margin: 20px 0; }
        .table th, .table td { padding: 12px; text-align: left; border-bottom: 1px solid #e0e0e0; }
        .table th { background: #f8f9fa; font-weight: 600; }
    </style>
</head>
<body>
    <div class="container">
        <div class="header">
            <h1>🚀 WRFCoin Integration Test Automation</h1>
            <p>Generated on ${new Date(reportData.timestamp).toLocaleString()}</p>
        </div>
        
        <div class="summary">
            <div class="metric">
                <h3>Total Tests</h3>
                <div class="value">${reportData.results.totalTests}</div>
            </div>
            <div class="metric">
                <h3>Passed</h3>
                <div class="value">${reportData.results.passedTests}</div>
            </div>
            <div class="metric">
                <h3>Failed</h3>
                <div class="value">${reportData.results.failedTests}</div>
            </div>
            <div class="metric">
                <h3>Success Rate</h3>
                <div class="value">${reportData.summary.successRate}%</div>
            </div>
        </div>
        
        <h2>📊 Test Suite Results</h2>
        ${reportData.results.suiteResults
          .map(
            suite => `
            <div class="suite ${suite.status === 'completed' && suite.failedTests === 0 ? 'passed' : 'failed'}">
                <h3>${suite.name}</h3>
                <p><strong>Status:</strong> ${suite.status}</p>
                <p><strong>Duration:</strong> ${suite.duration}ms</p>
                <p><strong>Results:</strong> ${suite.passedTests}/${suite.totalTests} passed</p>
                ${suite.error ? `<p><strong>Error:</strong> ${suite.error}</p>` : ''}
            </div>
        `
          )
          .join('')}
        
        <div class="performance">
            <h2>⚡ Performance Metrics</h2>
            <table class="table">
                <thead>
                    <tr>
                        <th>Metric</th>
                        <th>Value</th>
                    </tr>
                </thead>
                <tbody>
                    <tr>
                        <td>Total Duration</td>
                        <td>${reportData.results.performanceMetrics.orchestration?.totalDuration || 'N/A'}ms</td>
                    </tr>
                    <tr>
                        <td>Average Test Duration</td>
                        <td>${reportData.results.performanceMetrics.orchestration?.avgTestDuration || 'N/A'}ms</td>
                    </tr>
                    <tr>
                        <td>Success Rate</td>
                        <td>${reportData.results.performanceMetrics.orchestration?.successRate || 'N/A'}%</td>
                    </tr>
                </tbody>
            </table>
        </div>
        
        <h2>🏥 Service Health</h2>
        <table class="table">
            <thead>
                <tr>
                    <th>Service</th>
                    <th>Status</th>
                    <th>Details</th>
                </tr>
            </thead>
            <tbody>
                ${Object.entries(reportData.results.serviceHealthChecks)
                  .map(
                    ([service, health]) => `
                    <tr>
                        <td>${service}</td>
                        <td>${health.status}</td>
                        <td>${health.error || 'Healthy'}</td>
                    </tr>
                `
                  )
                  .join('')}
            </tbody>
        </table>
    </div>
</body>
</html>`;
  }

  /**
   * Generate JUnit XML report
   */
  generateJunitReport(reportData) {
    const totalTime =
      reportData.results.performanceMetrics.orchestration?.totalDuration /
        1000 || 0;

    return `<?xml version="1.0" encoding="UTF-8"?>
<testsuites name="WRFCoin Integration Test Automation" tests="${reportData.results.totalTests}" failures="${reportData.results.failedTests}" time="${totalTime}">
  ${reportData.results.suiteResults
    .map(
      suite => `
    <testsuite name="${suite.name}" tests="${suite.totalTests}" failures="${suite.failedTests}" time="${suite.duration / 1000}">
      ${suite.error ? `<error message="${suite.error}"></error>` : ''}
    </testsuite>
  `
    )
    .join('')}
</testsuites>`;
  }

  /**
   * Generate execution summary
   */
  generateSummary() {
    const duration = this.results.endTime
      ? this.results.endTime.getTime() - this.results.startTime.getTime()
      : Date.now() - this.results.startTime.getTime();

    const successRate =
      this.results.totalTests > 0
        ? Math.round((this.results.passedTests / this.results.totalTests) * 100)
        : 0;

    return {
      totalDuration: duration,
      successRate,
      suitesExecuted: this.results.suiteResults.length,
      servicesHealthy: Object.values(this.results.serviceHealthChecks).filter(
        health => health.status === 'healthy'
      ).length,
      totalServices: Object.keys(this.results.serviceHealthChecks).length,
      hasErrors: this.results.errors.length > 0,
      errorCount: this.results.errors.length,
    };
  }

  /**
   * Cleanup resources
   */
  async cleanup() {
    console.log('🧹 Cleaning up test environment...');

    try {
      // Stop Docker services
      const dockerComposeCmd = `docker-compose -f ${this.config.dockerComposeFile} --profile ${this.config.dockerComposeProfile} down`;
      await exec(dockerComposeCmd, { timeout: 30000 });
      console.log('✅ Docker services stopped');
    } catch (error) {
      console.warn('Failed to cleanup Docker services:', error.message);
    }
  }
}

module.exports = IntegrationTestOrchestrator;
