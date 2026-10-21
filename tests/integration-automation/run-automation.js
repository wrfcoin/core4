#!/usr/bin/env node
/**
 * Enhanced Integration Test Automation Runner
 * Provides comprehensive automation for integration test execution with CI/CD integration
 */

const { spawn, exec } = require('child_process');
const fs = require('fs');
const path = require('path');
const yargs = require('yargs');
const { promisify } = require('util');

const IntegrationTestOrchestrator = require('./utils/integration-test-orchestrator');

const execAsync = promisify(exec);

// CLI Configuration
const argv = yargs
  .option('config', {
    alias: 'c',
    description: 'Configuration file path',
    type: 'string',
    default: 'tests/integration-automation/config/automation.config.json',
  })
  .option('profile', {
    alias: 'p',
    description: 'Test execution profile',
    choices: ['fast', 'standard', 'comprehensive', 'ci'],
    default: 'standard',
  })
  .option('services', {
    alias: 's',
    description: 'Mock services to start',
    type: 'array',
    default: ['blockchain', 'weather', 'carbon'],
  })
  .option('parallel', {
    description: 'Number of parallel workers',
    type: 'number',
    default: 4,
  })
  .option('timeout', {
    alias: 't',
    description: 'Test timeout in milliseconds',
    type: 'number',
    default: 90000,
  })
  .option('retry', {
    alias: 'r',
    description: 'Number of retry attempts for failed tests',
    type: 'number',
    default: 2,
  })
  .option('watch', {
    alias: 'w',
    description: 'Watch mode for development',
    type: 'boolean',
    default: false,
  })
  .option('coverage', {
    description: 'Enable code coverage collection',
    type: 'boolean',
    default: false,
  })
  .option('reporter', {
    description: 'Additional reporters to use',
    type: 'array',
    default: ['automation', 'html', 'junit'],
  })
  .option('bail', {
    alias: 'b',
    description: 'Stop on first test failure',
    type: 'boolean',
    default: false,
  })
  .option('verbose', {
    alias: 'v',
    description: 'Verbose output',
    type: 'boolean',
    default: false,
  })
  .option('ci', {
    description: 'CI/CD mode with optimized settings',
    type: 'boolean',
    default: process.env.CI === 'true',
  })
  .option('docker-reset', {
    description: 'Reset Docker containers before starting',
    type: 'boolean',
    default: false,
  })
  .option('skip-services', {
    description: 'Skip starting mock services (assume running)',
    type: 'boolean',
    default: false,
  })
  .option('filter', {
    alias: 'f',
    description: 'Filter tests by pattern',
    type: 'string',
  })
  .option('exclude', {
    alias: 'e',
    description: 'Exclude tests by pattern',
    type: 'string',
  })
  .option('output-dir', {
    description: 'Output directory for results',
    type: 'string',
    default: 'tests/results/integration-automation',
  })
  .help()
  .alias('help', 'h')
  .version('1.0.0')
  .example('$0', 'Run integration tests with default settings')
  .example(
    '$0 --profile comprehensive --coverage',
    'Run comprehensive test suite with coverage'
  )
  .example('$0 --ci --parallel 2', 'Run in CI mode with 2 parallel workers')
  .example(
    '$0 --watch --filter "api-integration"',
    'Watch mode for API integration tests'
  ).argv;

class IntegrationTestAutomationRunner {
  constructor(options) {
    this.options = {
      profile: 'standard',
      parallel: 4,
      timeout: 90000,
      retry: 2,
      services: ['blockchain', 'weather', 'carbon'],
      coverage: false,
      watch: false,
      verbose: false,
      ci: false,
      dockerReset: false,
      skipServices: false,
      bail: false,
      outputDir: 'tests/results/integration-automation',
      ...options,
    };

    this.profiles = {
      fast: {
        timeout: 30000,
        parallel: 6,
        retry: 1,
        coverage: false,
        services: ['blockchain'],
        testPattern:
          'tests/integration-automation/suites/api-integration.test.js',
      },
      standard: {
        timeout: 90000,
        parallel: 4,
        retry: 2,
        coverage: false,
        services: ['blockchain', 'weather', 'carbon'],
        testPattern: 'tests/integration-automation/suites/**/*.test.js',
      },
      comprehensive: {
        timeout: 180000,
        parallel: 2,
        retry: 3,
        coverage: true,
        services: ['blockchain', 'weather', 'carbon', 'mockserver'],
        testPattern: 'tests/integration-automation/**/*.test.js',
      },
      ci: {
        timeout: 120000,
        parallel: 2,
        retry: 2,
        coverage: true,
        services: ['blockchain', 'weather', 'carbon'],
        testPattern: 'tests/integration-automation/suites/**/*.test.js',
        dockerReset: true,
        verbose: true,
      },
    };

    // Apply profile settings
    if (this.options.profile && this.profiles[this.options.profile]) {
      this.options = {
        ...this.options,
        ...this.profiles[this.options.profile],
      };
    }

    // CI-specific optimizations
    if (this.options.ci) {
      this.options.dockerReset = true;
      this.options.parallel = Math.min(this.options.parallel, 2);
      this.options.verbose = true;
      process.env.CI = 'true';
      process.env.INTEGRATION_COVERAGE = this.options.coverage
        ? 'true'
        : 'false';
    }

    this.orchestrator = null;
    this.startTime = null;
    this.results = null;
  }

  /**
   * Main execution method
   */
  async run() {
    try {
      console.log('🚀 Starting Integration Test Automation Runner');
      console.log(`📋 Profile: ${this.options.profile}`);
      console.log(`⚡ Parallel Workers: ${this.options.parallel}`);
      console.log(`⏱️  Timeout: ${this.options.timeout / 1000}s`);
      console.log(`🔄 Retry Attempts: ${this.options.retry}`);
      console.log(`🏥 Services: ${this.options.services.join(', ')}`);

      this.startTime = Date.now();

      // Pre-execution setup
      await this.setupEnvironment();

      // Docker container management
      if (!this.options.skipServices) {
        await this.manageDockerServices();
      }

      // Execute tests based on mode
      if (this.options.watch) {
        await this.runWatchMode();
      } else {
        await this.runSingleExecution();
      }

      // Post-execution tasks
      await this.generateReports();
      await this.cleanupResources();

      // Final summary
      this.printExecutionSummary();

      // Exit with appropriate code
      const success =
        this.results &&
        this.results.summary &&
        this.results.summary.successRate >= 95;
      process.exit(success ? 0 : 1);
    } catch (error) {
      console.error('❌ Automation runner failed:', error.message);
      if (this.options.verbose) {
        console.error(error.stack);
      }

      await this.cleanupResources();
      process.exit(1);
    }
  }

  /**
   * Setup test environment
   */
  async setupEnvironment() {
    console.log('🔧 Setting up test environment...');

    // Create output directories
    if (!fs.existsSync(this.options.outputDir)) {
      fs.mkdirSync(this.options.outputDir, { recursive: true });
    }

    // Validate prerequisites
    await this.validatePrerequisites();

    // Load configuration file if exists
    await this.loadConfiguration();

    // Set environment variables
    this.setEnvironmentVariables();

    console.log('✅ Environment setup complete');
  }

  /**
   * Validate prerequisites
   */
  async validatePrerequisites() {
    const requirements = [
      { command: 'node --version', name: 'Node.js' },
      { command: 'npm --version', name: 'npm' },
    ];

    if (!this.options.skipServices) {
      requirements.push(
        { command: 'docker --version', name: 'Docker' },
        { command: 'docker-compose --version', name: 'Docker Compose' }
      );
    }

    for (const req of requirements) {
      try {
        await execAsync(req.command);
        console.log(`✅ ${req.name} available`);
      } catch (error) {
        throw new Error(`❌ ${req.name} not available: ${error.message}`);
      }
    }
  }

  /**
   * Load configuration file
   */
  async loadConfiguration() {
    if (this.options.config && fs.existsSync(this.options.config)) {
      try {
        const configData = JSON.parse(
          fs.readFileSync(this.options.config, 'utf8')
        );
        this.options = { ...this.options, ...configData };
        console.log(`📋 Configuration loaded from: ${this.options.config}`);
      } catch (error) {
        console.warn(`⚠️  Failed to load configuration: ${error.message}`);
      }
    }
  }

  /**
   * Set environment variables
   */
  setEnvironmentVariables() {
    // Jest environment variables
    process.env.NODE_ENV = 'test';
    process.env.INTEGRATION_TEST_MODE = 'true';
    process.env.INTEGRATION_TEST_PROFILE = this.options.profile;
    process.env.INTEGRATION_TEST_PARALLEL_WORKERS =
      this.options.parallel.toString();
    process.env.INTEGRATION_TEST_TIMEOUT = this.options.timeout.toString();

    // Mock service URLs
    process.env.BLOCKCHAIN_RPC_URL = 'http://localhost:8545';
    process.env.WEATHER_API_URL = 'http://localhost:3010';
    process.env.CARBON_ORACLE_URL = 'http://localhost:3020';
    process.env.MOCKSERVER_URL = 'http://localhost:1080';

    // Database URLs
    process.env.DATABASE_URL =
      'postgresql://postgres:testpass@localhost:5432/wrfcoin_test';
    process.env.REDIS_URL = 'redis://localhost:6379';

    // Coverage settings
    if (this.options.coverage) {
      process.env.INTEGRATION_COVERAGE = 'true';
    }

    console.log('🌍 Environment variables configured');
  }

  /**
   * Manage Docker services
   */
  async manageDockerServices() {
    console.log('🐳 Managing Docker services...');

    try {
      // Reset containers if requested
      if (this.options.dockerReset) {
        console.log('🔄 Resetting Docker containers...');
        await execAsync(
          'docker-compose -f tests/docker-compose.test.yml --profile testing down -v'
        );
        await new Promise(resolve => setTimeout(resolve, 2000));
      }

      // Start services
      const composeCmd = `docker-compose -f tests/docker-compose.test.yml --profile testing up -d ${this.options.services.join(' ')}`;
      console.log(`Executing: ${composeCmd}`);

      await execAsync(composeCmd, { timeout: 120000 });
      console.log('✅ Docker services started');

      // Wait for services to be ready
      await this.waitForServices();
    } catch (error) {
      throw new Error(`Docker service management failed: ${error.message}`);
    }
  }

  /**
   * Wait for services to be ready
   */
  async waitForServices() {
    console.log('⏳ Waiting for services to be ready...');

    const serviceUrls = {
      blockchain: 'http://localhost:8545/health',
      weather: 'http://localhost:3010/health',
      carbon: 'http://localhost:3020/health',
    };

    const maxAttempts = 30;
    const delayBetweenAttempts = 2000;

    for (let attempt = 1; attempt <= maxAttempts; attempt++) {
      try {
        const fetch = (await import('node-fetch')).default;
        const healthChecks = await Promise.allSettled(
          this.options.services.map(async service => {
            if (serviceUrls[service]) {
              const response = await fetch(serviceUrls[service], {
                timeout: 3000,
              });
              return response.ok;
            }
            return true; // Skip unknown services
          })
        );

        const allHealthy = healthChecks.every(
          check => check.status === 'fulfilled' && check.value === true
        );

        if (allHealthy) {
          console.log('✅ All services are ready');
          return;
        }

        console.log(
          `⏳ Attempt ${attempt}/${maxAttempts}: Services not ready yet...`
        );
        await new Promise(resolve => setTimeout(resolve, delayBetweenAttempts));
      } catch (error) {
        console.log(`⏳ Attempt ${attempt}/${maxAttempts}: ${error.message}`);
        await new Promise(resolve => setTimeout(resolve, delayBetweenAttempts));
      }
    }

    throw new Error('Services failed to become ready within timeout period');
  }

  /**
   * Run single test execution
   */
  async runSingleExecution() {
    console.log('🧪 Running integration tests...');

    // Create orchestrator with current configuration
    const orchestratorConfig = {
      maxParallelWorkers: this.options.parallel,
      testTimeout: this.options.timeout,
      retryAttempts: this.options.retry,
      mockServicesEnabled: !this.options.skipServices,
      generateReports: true,
      reportFormats: this.options.reporter,
      services: {
        blockchain: 'http://localhost:8545',
        weather: 'http://localhost:3010',
        carbon: 'http://localhost:3020',
        mockserver: 'http://localhost:1080',
      },
    };

    this.orchestrator = new IntegrationTestOrchestrator(orchestratorConfig);
    this.results = await this.orchestrator.orchestrateTests();

    console.log('✅ Test execution completed');
  }

  /**
   * Run watch mode
   */
  async runWatchMode() {
    console.log('👀 Starting watch mode...');

    const jestArgs = [
      '--config=tests/jest.integration.config.js',
      '--watch',
      '--watchAll=false',
      `--maxWorkers=${this.options.parallel}`,
      `--testTimeout=${this.options.timeout}`,
    ];

    if (this.options.filter) {
      jestArgs.push(`--testNamePattern=${this.options.filter}`);
    }

    if (this.options.verbose) {
      jestArgs.push('--verbose');
    }

    const jestProcess = spawn('npx', ['jest', ...jestArgs], {
      stdio: 'inherit',
      env: { ...process.env },
    });

    return new Promise((resolve, reject) => {
      jestProcess.on('close', code => {
        if (code === 0) {
          resolve();
        } else {
          reject(new Error(`Jest watch mode exited with code ${code}`));
        }
      });

      jestProcess.on('error', reject);

      // Handle graceful shutdown
      process.on('SIGINT', () => {
        console.log('\n🛑 Shutting down watch mode...');
        jestProcess.kill('SIGINT');
      });
    });
  }

  /**
   * Generate additional reports
   */
  async generateReports() {
    console.log('📋 Generating additional reports...');

    try {
      // Generate execution summary
      const summaryReport = {
        timestamp: new Date().toISOString(),
        profile: this.options.profile,
        configuration: this.options,
        results: this.results,
        execution: {
          startTime: this.startTime,
          endTime: Date.now(),
          totalDuration: Date.now() - this.startTime,
        },
      };

      const summaryPath = path.join(
        this.options.outputDir,
        'execution-summary.json'
      );
      fs.writeFileSync(summaryPath, JSON.stringify(summaryReport, null, 2));
      console.log(`📄 Execution summary: ${summaryPath}`);

      // Generate CI/CD artifacts if in CI mode
      if (this.options.ci) {
        await this.generateCIArtifacts(summaryReport);
      }
    } catch (error) {
      console.warn('⚠️  Failed to generate additional reports:', error.message);
    }
  }

  /**
   * Generate CI/CD artifacts
   */
  async generateCIArtifacts(summaryReport) {
    console.log('🔧 Generating CI/CD artifacts...');

    // GitHub Actions summary
    if (process.env.GITHUB_ACTIONS) {
      const githubSummary = this.generateGitHubSummary(summaryReport);
      const summaryPath = path.join(
        this.options.outputDir,
        'github-summary.md'
      );
      fs.writeFileSync(summaryPath, githubSummary);

      // Set GitHub Actions outputs
      if (process.env.GITHUB_OUTPUT) {
        const outputs = [
          `success=${summaryReport.results?.summary?.successRate >= 95 ? 'true' : 'false'}`,
          `success-rate=${summaryReport.results?.summary?.successRate || 0}`,
          `total-tests=${summaryReport.results?.summary?.suitesExecuted || 0}`,
          `duration=${summaryReport.execution?.totalDuration || 0}`,
        ];

        fs.appendFileSync(process.env.GITHUB_OUTPUT, outputs.join('\n') + '\n');
      }
    }

    // Badge generation
    const badgeData = {
      schemaVersion: 1,
      label: 'integration-tests',
      message: `${summaryReport.results?.summary?.successRate?.toFixed(1) || 0}% passed`,
      color:
        (summaryReport.results?.summary?.successRate || 0) >= 95
          ? 'brightgreen'
          : 'red',
    };

    const badgePath = path.join(this.options.outputDir, 'test-badge.json');
    fs.writeFileSync(badgePath, JSON.stringify(badgeData, null, 2));

    console.log('✅ CI/CD artifacts generated');
  }

  /**
   * Generate GitHub Actions summary
   */
  generateGitHubSummary(summaryReport) {
    const successRate = summaryReport.results?.summary?.successRate || 0;
    const emoji = successRate >= 95 ? '✅' : successRate >= 80 ? '⚠️' : '❌';

    return `# ${emoji} Integration Test Automation Report

## 📊 Summary
- **Profile**: ${summaryReport.profile}
- **Success Rate**: ${successRate.toFixed(1)}%
- **Total Duration**: ${((summaryReport.execution?.totalDuration || 0) / 1000).toFixed(1)}s
- **Suites Executed**: ${summaryReport.results?.summary?.suitesExecuted || 0}

## 🎯 Results
${
  summaryReport.results?.summary
    ? `
- ✅ Passed: ${summaryReport.results.summary.servicesHealthy || 0}
- ❌ Failed: ${(summaryReport.results.summary.totalServices || 0) - (summaryReport.results.summary.servicesHealthy || 0)}
- 🏥 Services Healthy: ${summaryReport.results.summary.servicesHealthy || 0}/${summaryReport.results.summary.totalServices || 0}
`
    : 'No detailed results available'
}

## ⚡ Performance
- **Average Duration**: ${(summaryReport.execution?.totalDuration || 0) / 1000}s
- **Error Count**: ${summaryReport.results?.summary?.errorCount || 0}

---
*Generated by WRFCoin Integration Test Automation Runner*`;
  }

  /**
   * Cleanup resources
   */
  async cleanupResources() {
    console.log('🧹 Cleaning up resources...');

    try {
      if (!this.options.skipServices && !this.options.watch) {
        console.log('🐳 Stopping Docker services...');
        await execAsync(
          'docker-compose -f tests/docker-compose.test.yml --profile testing down',
          {
            timeout: 30000,
          }
        );
        console.log('✅ Docker services stopped');
      }
    } catch (error) {
      console.warn('⚠️  Cleanup warning:', error.message);
    }
  }

  /**
   * Print execution summary
   */
  printExecutionSummary() {
    const duration = Date.now() - this.startTime;
    const successRate = this.results?.summary?.successRate || 0;

    console.log('\n' + '='.repeat(60));
    console.log('🎯 INTEGRATION TEST AUTOMATION COMPLETE');
    console.log('='.repeat(60));
    console.log(`📋 Profile: ${this.options.profile}`);
    console.log(`⏱️  Total Duration: ${(duration / 1000).toFixed(1)}s`);
    console.log(`🎯 Success Rate: ${successRate.toFixed(1)}%`);
    console.log(`🏆 Grade: ${this.getGrade(successRate)}`);

    if (this.results?.summary?.errorCount > 0) {
      console.log(`⚠️  Errors: ${this.results.summary.errorCount}`);
    }

    console.log(`📄 Reports: ${this.options.outputDir}`);
    console.log('='.repeat(60));

    const status = successRate >= 95 ? '🎉 SUCCESS' : '⚠️  NEEDS ATTENTION';
    console.log(status);
    console.log('='.repeat(60) + '\n');
  }

  /**
   * Get grade based on success rate
   */
  getGrade(successRate) {
    if (successRate >= 98) return 'A+';
    if (successRate >= 95) return 'A';
    if (successRate >= 90) return 'B';
    if (successRate >= 80) return 'C';
    return 'D';
  }
}

// Execute if called directly
if (require.main === module) {
  const runner = new IntegrationTestAutomationRunner(argv);
  runner.run().catch(console.error);
}

module.exports = IntegrationTestAutomationRunner;
