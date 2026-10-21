#!/usr/bin/env node

const { spawn, exec } = require('child_process');
const axios = require('axios');
const fs = require('fs').promises;
const path = require('path');
const yaml = require('yaml');

/**
 * Integration Test Runner
 *
 * Coordinates comprehensive integration testing with mock services
 * Features:
 * - Service orchestration and health monitoring
 * - Test scenario management
 * - Parallel test execution
 * - Comprehensive reporting
 * - Cleanup and teardown
 */

class IntegrationTestRunner {
  constructor(options = {}) {
    this.config = {
      dockerCompose: options.dockerCompose || 'docker-compose.test.yml',
      healthCheckerUrl: options.healthCheckerUrl || 'http://localhost:3100',
      testDir: options.testDir || './tests',
      reportsDir: options.reportsDir || './reports',
      timeout: options.timeout || 300000, // 5 minutes
      retries: options.retries || 2,
      profile: options.profile || 'integration-test',
      parallel: options.parallel || 4,
      verbose: options.verbose || false,
    };

    this.results = {
      startTime: null,
      endTime: null,
      duration: 0,
      scenarios: [],
      services: [],
      tests: [],
      summary: {
        total: 0,
        passed: 0,
        failed: 0,
        skipped: 0,
        errors: 0,
      },
    };

    this.services = new Map();
    this.scenarios = new Map();
  }

  // Logging utilities
  log(message, level = 'info') {
    const timestamp = new Date().toISOString();
    const prefix = `[${timestamp}] [${level.toUpperCase()}]`;

    if (level === 'error') {
      console.error(`${prefix} ${message}`);
    } else if (level === 'warn') {
      console.warn(`${prefix} ${message}`);
    } else if (level === 'debug' && this.config.verbose) {
      console.log(`${prefix} ${message}`);
    } else if (level === 'info') {
      console.log(`${prefix} ${message}`);
    }
  }

  // Execute shell commands with promise interface
  execCommand(command, options = {}) {
    return new Promise((resolve, reject) => {
      const child = exec(command, {
        cwd: process.cwd(),
        env: { ...process.env, ...options.env },
        maxBuffer: 1024 * 1024 * 10, // 10MB buffer
        ...options,
      });

      let stdout = '';
      let stderr = '';

      child.stdout?.on('data', data => {
        stdout += data;
        if (this.config.verbose) {
          process.stdout.write(data);
        }
      });

      child.stderr?.on('data', data => {
        stderr += data;
        if (this.config.verbose) {
          process.stderr.write(data);
        }
      });

      child.on('close', code => {
        if (code === 0) {
          resolve({ stdout, stderr, code });
        } else {
          reject(
            new Error(`Command failed with code ${code}: ${command}\n${stderr}`)
          );
        }
      });

      child.on('error', reject);
    });
  }

  // Load test scenarios from configuration
  async loadScenarios() {
    try {
      const scenarioFiles = [
        './mocks/weather-scenarios.json',
        './mocks/blockchain-scenarios.json',
        './mocks/carbon-scenarios.json',
      ];

      for (const file of scenarioFiles) {
        try {
          const content = await fs.readFile(file, 'utf8');
          const scenarios = JSON.parse(content);
          const scenarioType = path
            .basename(file, '.json')
            .replace('-scenarios', '');

          Object.entries(scenarios).forEach(([name, config]) => {
            this.scenarios.set(`${scenarioType}-${name}`, {
              type: scenarioType,
              name,
              config,
              file,
            });
          });
        } catch (error) {
          this.log(
            `Warning: Could not load scenarios from ${file}: ${error.message}`,
            'warn'
          );
        }
      }

      this.log(`Loaded ${this.scenarios.size} test scenarios`);
    } catch (error) {
      this.log(`Error loading scenarios: ${error.message}`, 'error');
    }
  }

  // Start Docker Compose services
  async startServices() {
    this.log('Starting Docker Compose services...');

    try {
      const command = `docker-compose -f ${this.config.dockerCompose} --profile ${this.config.profile} up -d --build`;
      await this.execCommand(command, { timeout: 120000 }); // 2 minutes timeout

      this.log('Docker services started, waiting for health checks...');
      return true;
    } catch (error) {
      this.log(`Failed to start services: ${error.message}`, 'error');
      return false;
    }
  }

  // Wait for services to be ready
  async waitForServices(timeout = 120000) {
    const startTime = Date.now();
    const checkInterval = 5000; // 5 seconds

    this.log('Waiting for services to become ready...');

    while (Date.now() - startTime < timeout) {
      try {
        const response = await axios.get(
          `${this.config.healthCheckerUrl}/health/ready`,
          {
            timeout: 5000,
            params: { services: 'postgres,redis,weather-mock,blockchain-mock' },
          }
        );

        if (response.data.ready) {
          this.log(
            `All required services are ready (${response.data.readyCount}/${response.data.totalRequired})`
          );
          return true;
        } else {
          const notReady = response.data.services
            .filter(s => !s.ready)
            .map(s => s.name)
            .join(', ');
          this.log(`Services not ready: ${notReady}`, 'debug');
        }
      } catch (error) {
        this.log(`Health check failed: ${error.message}`, 'debug');
      }

      await new Promise(resolve => setTimeout(resolve, checkInterval));
    }

    this.log('Timeout waiting for services to be ready', 'error');
    return false;
  }

  // Get service health status
  async getServiceHealth() {
    try {
      const response = await axios.get(
        `${this.config.healthCheckerUrl}/health/services`
      );
      return response.data.services;
    } catch (error) {
      this.log(`Failed to get service health: ${error.message}`, 'warn');
      return [];
    }
  }

  // Discover test files
  async discoverTests() {
    const testFiles = [];
    const testPattern = /\.(test|spec)\.(js|ts)$/;

    try {
      const files = await this.walkDirectory(this.config.testDir);

      for (const file of files) {
        if (testPattern.test(file)) {
          testFiles.push(file);
        }
      }
    } catch (error) {
      this.log(`Error discovering tests: ${error.message}`, 'warn');
    }

    this.log(`Discovered ${testFiles.length} test files`);
    return testFiles;
  }

  // Recursively walk directory
  async walkDirectory(dir) {
    const files = [];

    try {
      const entries = await fs.readdir(dir, { withFileTypes: true });

      for (const entry of entries) {
        const fullPath = path.join(dir, entry.name);

        if (entry.isDirectory()) {
          const subFiles = await this.walkDirectory(fullPath);
          files.push(...subFiles);
        } else {
          files.push(fullPath);
        }
      }
    } catch (error) {
      // Directory might not exist, that's ok
      this.log(`Cannot read directory ${dir}: ${error.message}`, 'debug');
    }

    return files;
  }

  // Run specific test suite
  async runTestSuite(testFile, scenario = null) {
    const testName = path.relative(this.config.testDir, testFile);
    this.log(
      `Running test suite: ${testName}${scenario ? ` (scenario: ${scenario})` : ''}`
    );

    const testResult = {
      file: testFile,
      name: testName,
      scenario,
      startTime: Date.now(),
      endTime: null,
      duration: 0,
      status: 'unknown',
      passed: 0,
      failed: 0,
      skipped: 0,
      output: '',
      error: null,
    };

    try {
      const env = {
        NODE_ENV: 'test',
        MOCK_SCENARIO: scenario || 'default',
        HEALTH_CHECKER_URL: this.config.healthCheckerUrl,
        ...process.env,
      };

      // Detect test framework and run accordingly
      let command;
      if (testFile.includes('jest') || (await this.hasJestConfig())) {
        command = `npx jest ${testFile} --detectOpenHandles --forceExit`;
      } else if (testFile.includes('mocha') || (await this.hasMochaConfig())) {
        command = `npx mocha ${testFile} --timeout 30000`;
      } else {
        // Default to Node.js execution
        command = `node ${testFile}`;
      }

      const result = await this.execCommand(command, {
        env,
        timeout: this.config.timeout,
      });

      testResult.status = 'passed';
      testResult.output = result.stdout;
      testResult.passed = this.extractTestCounts(result.stdout).passed;
      testResult.failed = this.extractTestCounts(result.stdout).failed;
      testResult.skipped = this.extractTestCounts(result.stdout).skipped;
    } catch (error) {
      testResult.status = 'failed';
      testResult.error = error.message;
      testResult.output = error.message;
      testResult.failed = 1;
    }

    testResult.endTime = Date.now();
    testResult.duration = testResult.endTime - testResult.startTime;

    return testResult;
  }

  // Extract test counts from test runner output
  extractTestCounts(output) {
    const counts = { passed: 0, failed: 0, skipped: 0 };

    // Jest patterns
    let match = output.match(/(\d+) passed/);
    if (match) counts.passed = parseInt(match[1]);

    match = output.match(/(\d+) failed/);
    if (match) counts.failed = parseInt(match[1]);

    match = output.match(/(\d+) skipped/);
    if (match) counts.skipped = parseInt(match[1]);

    // Mocha patterns
    match = output.match(/(\d+) passing/);
    if (match) counts.passed = parseInt(match[1]);

    match = output.match(/(\d+) failing/);
    if (match) counts.failed = parseInt(match[1]);

    return counts;
  }

  // Check for test framework configurations
  async hasJestConfig() {
    const jestFiles = ['jest.config.js', 'jest.config.json', 'package.json'];
    for (const file of jestFiles) {
      try {
        const content = await fs.readFile(file, 'utf8');
        if (file === 'package.json') {
          const pkg = JSON.parse(content);
          return !!pkg.jest;
        }
        return true;
      } catch (error) {
        continue;
      }
    }
    return false;
  }

  async hasMochaConfig() {
    const mochaFiles = ['.mocharc.json', '.mocharc.yml', '.mocharc.yaml'];
    for (const file of mochaFiles) {
      try {
        await fs.access(file);
        return true;
      } catch (error) {
        continue;
      }
    }
    return false;
  }

  // Run all integration tests
  async runAllTests() {
    this.results.startTime = Date.now();

    try {
      // Discover test files
      const testFiles = await this.discoverTests();
      if (testFiles.length === 0) {
        this.log('No test files found', 'warn');
        return this.results;
      }

      // Run tests in batches (parallel execution)
      const batches = this.chunkArray(testFiles, this.config.parallel);

      for (let i = 0; i < batches.length; i++) {
        const batch = batches[i];
        this.log(
          `Running test batch ${i + 1}/${batches.length} (${batch.length} tests)`
        );

        const batchPromises = batch.map(testFile =>
          this.runTestSuite(testFile).catch(error => ({
            file: testFile,
            name: path.relative(this.config.testDir, testFile),
            status: 'error',
            error: error.message,
            passed: 0,
            failed: 1,
            skipped: 0,
            duration: 0,
          }))
        );

        const batchResults = await Promise.all(batchPromises);
        this.results.tests.push(...batchResults);

        // Update summary statistics
        batchResults.forEach(result => {
          this.results.summary.total++;
          if (result.status === 'passed') this.results.summary.passed++;
          else if (result.status === 'failed') this.results.summary.failed++;
          else if (result.status === 'skipped') this.results.summary.skipped++;
          else this.results.summary.errors++;
        });

        this.log(
          `Batch ${i + 1} completed: ${batchResults.filter(r => r.status === 'passed').length}/${batchResults.length} passed`
        );
      }
    } catch (error) {
      this.log(`Test execution failed: ${error.message}`, 'error');
    }

    this.results.endTime = Date.now();
    this.results.duration = this.results.endTime - this.results.startTime;

    return this.results;
  }

  // Utility: chunk array into batches
  chunkArray(array, chunkSize) {
    const chunks = [];
    for (let i = 0; i < array.length; i += chunkSize) {
      chunks.push(array.slice(i, i + chunkSize));
    }
    return chunks;
  }

  // Generate comprehensive test report
  async generateReport() {
    try {
      // Ensure reports directory exists
      await fs.mkdir(this.config.reportsDir, { recursive: true });

      // Get final service health status
      this.results.services = await this.getServiceHealth();

      // Create detailed report
      const report = {
        metadata: {
          generatedAt: new Date().toISOString(),
          runner: 'integration-test-runner',
          version: '1.0.0',
          configuration: this.config,
        },
        execution: {
          startTime: new Date(this.results.startTime).toISOString(),
          endTime: new Date(this.results.endTime).toISOString(),
          duration: this.results.duration,
          durationFormatted: this.formatDuration(this.results.duration),
        },
        summary: this.results.summary,
        services: this.results.services.map(service => ({
          id: service.id,
          name: service.name,
          type: service.type,
          critical: service.critical,
          status: service.health?.status || 'unknown',
          responseTime: service.health?.responseTime,
          lastCheck: service.health?.lastCheck,
        })),
        scenarios: Array.from(this.scenarios.values()),
        tests: this.results.tests,
        environment: {
          nodeVersion: process.version,
          platform: process.platform,
          arch: process.arch,
          memory: process.memoryUsage(),
        },
      };

      // Save JSON report
      const jsonReportPath = path.join(
        this.config.reportsDir,
        'integration-test-report.json'
      );
      await fs.writeFile(jsonReportPath, JSON.stringify(report, null, 2));

      // Save HTML report
      const htmlReport = this.generateHtmlReport(report);
      const htmlReportPath = path.join(
        this.config.reportsDir,
        'integration-test-report.html'
      );
      await fs.writeFile(htmlReportPath, htmlReport);

      // Save JUnit XML for CI integration
      const junitReport = this.generateJunitReport(report);
      const junitReportPath = path.join(
        this.config.reportsDir,
        'integration-test-results.xml'
      );
      await fs.writeFile(junitReportPath, junitReport);

      this.log(`Reports generated in ${this.config.reportsDir}`);
      this.log(`  - JSON: ${jsonReportPath}`);
      this.log(`  - HTML: ${htmlReportPath}`);
      this.log(`  - JUnit XML: ${junitReportPath}`);

      return report;
    } catch (error) {
      this.log(`Failed to generate report: ${error.message}`, 'error');
      return null;
    }
  }

  // Format duration in human readable format
  formatDuration(ms) {
    const seconds = Math.floor(ms / 1000);
    const minutes = Math.floor(seconds / 60);
    const remainingSeconds = seconds % 60;

    if (minutes > 0) {
      return `${minutes}m ${remainingSeconds}s`;
    }
    return `${remainingSeconds}s`;
  }

  // Generate HTML report
  generateHtmlReport(report) {
    const { summary, tests, services } = report;
    const passRate =
      summary.total > 0
        ? ((summary.passed / summary.total) * 100).toFixed(1)
        : 0;

    return `
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Integration Test Report - WRFCoin</title>
    <style>
        body { font-family: Arial, sans-serif; margin: 40px; background: #f5f5f5; }
        .container { max-width: 1200px; margin: 0 auto; background: white; padding: 30px; border-radius: 8px; box-shadow: 0 2px 10px rgba(0,0,0,0.1); }
        .header { text-align: center; margin-bottom: 40px; }
        .summary { display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 20px; margin-bottom: 40px; }
        .summary-card { background: #f8f9fa; padding: 20px; border-radius: 8px; text-align: center; border-left: 4px solid #007bff; }
        .summary-card.passed { border-color: #28a745; }
        .summary-card.failed { border-color: #dc3545; }
        .summary-card.skipped { border-color: #ffc107; }
        .summary-card h3 { margin: 0 0 10px 0; color: #666; }
        .summary-card .value { font-size: 2em; font-weight: bold; color: #333; }
        .section { margin-bottom: 40px; }
        .section h2 { color: #333; border-bottom: 2px solid #007bff; padding-bottom: 10px; }
        table { width: 100%; border-collapse: collapse; margin-top: 20px; }
        th, td { padding: 12px; text-align: left; border-bottom: 1px solid #ddd; }
        th { background-color: #f8f9fa; font-weight: 600; }
        .status { padding: 4px 8px; border-radius: 4px; font-size: 0.85em; font-weight: 500; }
        .status.passed { background: #d4edda; color: #155724; }
        .status.failed { background: #f8d7da; color: #721c24; }
        .status.error { background: #f8d7da; color: #721c24; }
        .status.healthy { background: #d4edda; color: #155724; }
        .status.unhealthy { background: #f8d7da; color: #721c24; }
        .duration { color: #666; font-size: 0.9em; }
        .critical-badge { background: #dc3545; color: white; padding: 2px 6px; border-radius: 3px; font-size: 0.8em; }
    </style>
</head>
<body>
    <div class="container">
        <div class="header">
            <h1>WRFCoin Integration Test Report</h1>
            <p>Generated on ${new Date(report.metadata.generatedAt).toLocaleString()}</p>
            <p>Duration: ${report.execution.durationFormatted} | Pass Rate: ${passRate}%</p>
        </div>
        
        <div class="section">
            <div class="summary">
                <div class="summary-card">
                    <h3>Total Tests</h3>
                    <div class="value">${summary.total}</div>
                </div>
                <div class="summary-card passed">
                    <h3>Passed</h3>
                    <div class="value">${summary.passed}</div>
                </div>
                <div class="summary-card failed">
                    <h3>Failed</h3>
                    <div class="value">${summary.failed}</div>
                </div>
                <div class="summary-card skipped">
                    <h3>Skipped</h3>
                    <div class="value">${summary.skipped}</div>
                </div>
            </div>
        </div>
        
        <div class="section">
            <h2>Service Health Status</h2>
            <table>
                <thead>
                    <tr>
                        <th>Service</th>
                        <th>Type</th>
                        <th>Status</th>
                        <th>Response Time</th>
                        <th>Critical</th>
                    </tr>
                </thead>
                <tbody>
                    ${services
                      .map(
                        service => `
                        <tr>
                            <td>${service.name}</td>
                            <td>${service.type}</td>
                            <td><span class="status ${service.status}">${service.status}</span></td>
                            <td class="duration">${service.responseTime ? service.responseTime + 'ms' : 'N/A'}</td>
                            <td>${service.critical ? '<span class="critical-badge">Critical</span>' : ''}</td>
                        </tr>
                    `
                      )
                      .join('')}
                </tbody>
            </table>
        </div>
        
        <div class="section">
            <h2>Test Results</h2>
            <table>
                <thead>
                    <tr>
                        <th>Test Suite</th>
                        <th>Status</th>
                        <th>Passed</th>
                        <th>Failed</th>
                        <th>Skipped</th>
                        <th>Duration</th>
                    </tr>
                </thead>
                <tbody>
                    ${tests
                      .map(
                        test => `
                        <tr>
                            <td>${test.name}</td>
                            <td><span class="status ${test.status}">${test.status}</span></td>
                            <td>${test.passed}</td>
                            <td>${test.failed}</td>
                            <td>${test.skipped}</td>
                            <td class="duration">${this.formatDuration(test.duration)}</td>
                        </tr>
                    `
                      )
                      .join('')}
                </tbody>
            </table>
        </div>
    </div>
</body>
</html>`;
  }

  // Generate JUnit XML report
  generateJunitReport(report) {
    const { tests, summary } = report;

    let xml = `<?xml version="1.0" encoding="UTF-8"?>\n`;
    xml += `<testsuites name="WRFCoin Integration Tests" tests="${summary.total}" failures="${summary.failed}" errors="${summary.errors}" time="${(report.execution.duration / 1000).toFixed(2)}">\n`;

    tests.forEach(test => {
      xml += `  <testsuite name="${test.name}" tests="${test.passed + test.failed + test.skipped}" failures="${test.failed}" errors="${test.status === 'error' ? 1 : 0}" time="${(test.duration / 1000).toFixed(2)}">\n`;

      if (test.status === 'passed') {
        xml += `    <testcase name="${test.name}" time="${(test.duration / 1000).toFixed(2)}"/>\n`;
      } else if (test.status === 'failed') {
        xml += `    <testcase name="${test.name}" time="${(test.duration / 1000).toFixed(2)}">\n`;
        xml += `      <failure message="Test failed">${this.escapeXml(test.error || test.output)}</failure>\n`;
        xml += `    </testcase>\n`;
      } else if (test.status === 'error') {
        xml += `    <testcase name="${test.name}" time="${(test.duration / 1000).toFixed(2)}">\n`;
        xml += `      <error message="Test error">${this.escapeXml(test.error || 'Unknown error')}</error>\n`;
        xml += `    </testcase>\n`;
      }

      xml += `  </testsuite>\n`;
    });

    xml += `</testsuites>`;
    return xml;
  }

  // Escape XML special characters
  escapeXml(unsafe) {
    return unsafe
      .replace(/&/g, '&amp;')
      .replace(/</g, '&lt;')
      .replace(/>/g, '&gt;')
      .replace(/"/g, '&quot;')
      .replace(/'/g, '&#039;');
  }

  // Stop Docker Compose services
  async stopServices() {
    this.log('Stopping Docker Compose services...');

    try {
      await this.execCommand(
        `docker-compose -f ${this.config.dockerCompose} down -v`
      );
      this.log('Services stopped and volumes removed');
      return true;
    } catch (error) {
      this.log(`Failed to stop services: ${error.message}`, 'error');
      return false;
    }
  }

  // Main execution method
  async run() {
    this.log('Starting Integration Test Runner');
    this.log(
      `Configuration: profile=${this.config.profile}, parallel=${this.config.parallel}, timeout=${this.config.timeout}ms`
    );

    let success = false;

    try {
      // Load scenarios
      await this.loadScenarios();

      // Start services
      const servicesStarted = await this.startServices();
      if (!servicesStarted) {
        throw new Error('Failed to start required services');
      }

      // Wait for services to be ready
      const servicesReady = await this.waitForServices();
      if (!servicesReady) {
        throw new Error('Services did not become ready within timeout');
      }

      // Run tests
      await this.runAllTests();

      // Generate reports
      await this.generateReport();

      // Log final results
      const { summary } = this.results;
      this.log(`\n=== Integration Test Results ===`);
      this.log(
        `Total: ${summary.total}, Passed: ${summary.passed}, Failed: ${summary.failed}, Skipped: ${summary.skipped}, Errors: ${summary.errors}`
      );
      this.log(`Duration: ${this.formatDuration(this.results.duration)}`);
      this.log(
        `Pass Rate: ${summary.total > 0 ? ((summary.passed / summary.total) * 100).toFixed(1) : 0}%`
      );

      success = summary.failed === 0 && summary.errors === 0;
    } catch (error) {
      this.log(`Integration test run failed: ${error.message}`, 'error');
      success = false;
    } finally {
      // Always try to cleanup
      if (process.env.SKIP_CLEANUP !== 'true') {
        await this.stopServices();
      }
    }

    // Exit with appropriate code
    process.exit(success ? 0 : 1);
  }
}

// CLI interface
if (require.main === module) {
  const args = process.argv.slice(2);
  const options = {};

  // Parse command line arguments
  for (let i = 0; i < args.length; i++) {
    const arg = args[i];
    if (arg.startsWith('--')) {
      const [key, value] = arg.slice(2).split('=');
      options[key.replace(/-([a-z])/g, (_, letter) => letter.toUpperCase())] =
        value === undefined ? true : isNaN(value) ? value : parseInt(value);
    }
  }

  // Create and run test runner
  const runner = new IntegrationTestRunner(options);
  runner.run().catch(error => {
    console.error('Fatal error:', error);
    process.exit(1);
  });
}

module.exports = IntegrationTestRunner;
