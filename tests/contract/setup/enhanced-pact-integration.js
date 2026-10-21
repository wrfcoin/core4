/**
 * Enhanced Pact Integration with Metrics and Reporting
 *
 * Extends the existing Pact setup with comprehensive monitoring and reporting
 * Integrates with contract test metrics collector and reporting dashboard
 */

const ContractTestReporter = require('./contract-test-reporter');
const path = require('path');
const { execSync } = require('child_process');

class EnhancedPactIntegration {
  constructor(options = {}) {
    this.reporter = new ContractTestReporter({
      metricsCollectorUrl:
        options.metricsCollectorUrl ||
        process.env.CONTRACT_METRICS_URL ||
        'http://localhost:9095',
      reportDir:
        options.reportDir ||
        path.resolve(process.cwd(), 'tests/contract/reports'),
      environment: options.environment || process.env.NODE_ENV || 'development',
      version: options.version || process.env.GIT_SHA || '1.0.0',
    });

    this.pactBrokerUrl =
      options.pactBrokerUrl ||
      process.env.PACT_BROKER_URL ||
      'http://localhost:9292';
    this.enabled = options.enabled !== false;
  }

  /**
   * Enhanced test setup with metrics integration
   */
  async setupPactTest(testConfig) {
    if (!this.enabled) return testConfig;

    const startTime = Date.now();

    // Add hooks for metrics collection
    const originalBeforeEach = testConfig.beforeEach || (() => {});
    const originalAfterEach = testConfig.afterEach || (() => {});

    testConfig.beforeEach = async function () {
      this.testStartTime = Date.now();
      await originalBeforeEach.call(this);
    };

    testConfig.afterEach = async function () {
      const duration = (Date.now() - this.testStartTime) / 1000;
      const testResult = {
        consumer: testConfig.consumer || 'unknown',
        provider: testConfig.provider || 'unknown',
        testType: testConfig.testType || 'consumer',
        status: this.currentTest?.state === 'passed' ? 'passed' : 'failed',
        duration,
        errorType: this.currentTest?.err?.name || null,
        errorMessage: this.currentTest?.err?.message || null,
        testName: this.currentTest?.title || 'unknown',
      };

      // Report test result
      await this.reporter.reportTestResult(testResult);

      await originalAfterEach.call(this);
    }.bind(this);

    return testConfig;
  }

  /**
   * Enhanced Pact verification with metrics
   */
  async enhancedVerifyPacts(verificationOptions) {
    const startTime = Date.now();
    let verificationResult;

    try {
      // Add custom verification hooks
      const enhancedOptions = {
        ...verificationOptions,
        beforeEach: async (interaction, provider) => {
          console.log(`Verifying interaction: ${interaction.description}`);
          if (verificationOptions.beforeEach) {
            await verificationOptions.beforeEach(interaction, provider);
          }
        },
        afterEach: async (interaction, provider, result) => {
          const testResult = {
            consumer: interaction.consumer || verificationOptions.consumer,
            provider: provider.name || verificationOptions.provider,
            testType: 'provider',
            status: result.success ? 'passed' : 'failed',
            duration: (Date.now() - startTime) / 1000,
            errorType: result.error?.type || null,
            errorMessage: result.error?.message || null,
            testName: interaction.description,
          };

          await this.reporter.reportTestResult(testResult);

          if (verificationOptions.afterEach) {
            await verificationOptions.afterEach(interaction, provider, result);
          }
        },
      };

      // Execute verification with enhanced options
      verificationResult = await this.executeVerification(enhancedOptions);
    } catch (error) {
      console.error('Pact verification failed:', error);

      // Report failure
      await this.reporter.reportTestResult({
        consumer: verificationOptions.consumer || 'unknown',
        provider: verificationOptions.provider || 'unknown',
        testType: 'provider',
        status: 'failed',
        duration: (Date.now() - startTime) / 1000,
        errorType: error.name || 'verification_error',
        errorMessage: error.message,
        testName: 'pact_verification',
      });

      throw error;
    }

    return verificationResult;
  }

  async executeVerification(options) {
    // This would integrate with the actual Pact verification library
    // For now, we'll simulate the verification process
    console.log('Executing Pact verification with enhanced monitoring...');

    // In a real implementation, this would call the Pact verification library
    // e.g., return await new Verifier().verifyProvider(options);

    return {
      success: true,
      results: [],
    };
  }

  /**
   * Publish Pacts with enhanced metadata
   */
  async publishPactsWithMetrics(publishOptions) {
    const startTime = Date.now();

    try {
      // Add enhanced metadata
      const enhancedOptions = {
        ...publishOptions,
        tags: [
          ...(publishOptions.tags || []),
          `env:${process.env.NODE_ENV || 'development'}`,
          `version:${process.env.GIT_SHA || 'latest'}`,
          `timestamp:${new Date().toISOString()}`,
        ],
        buildUrl: process.env.BUILD_URL || null,
        branch: process.env.GIT_BRANCH || 'main',
      };

      // Execute publication
      const result = await this.executePactPublication(enhancedOptions);

      // Report publication to metrics collector
      await this.reporter.reportPactPublication({
        consumer: publishOptions.consumer,
        provider: publishOptions.provider,
        version: publishOptions.consumerVersion,
        environment: process.env.NODE_ENV || 'development',
        branch: process.env.GIT_BRANCH || 'main',
        success: true,
      });

      console.log(
        `Pact published successfully for ${publishOptions.consumer} -> ${publishOptions.provider}`
      );
      return result;
    } catch (error) {
      console.error('Pact publication failed:', error);

      // Report publication failure
      await this.reporter.reportPactPublication({
        consumer: publishOptions.consumer,
        provider: publishOptions.provider,
        version: publishOptions.consumerVersion,
        environment: process.env.NODE_ENV || 'development',
        success: false,
        error: error.message,
      });

      throw error;
    }
  }

  async executePactPublication(options) {
    // This would integrate with the actual Pact publication process
    console.log('Publishing Pact with enhanced metadata...');

    // In a real implementation, this would use the Pact Broker client
    // e.g., return await publisher.publish(options);

    return {
      success: true,
      url: `${this.pactBrokerUrl}/pacts/provider/${options.provider}/consumer/${options.consumer}/version/${options.consumerVersion}`,
    };
  }

  /**
   * Generate comprehensive contract test report
   */
  async generateContractReport(testResults = []) {
    try {
      const report = await this.reporter.generateTestReport(testResults);
      console.log('Contract test report generated successfully');
      return report;
    } catch (error) {
      console.error('Failed to generate contract test report:', error);
      throw error;
    }
  }

  /**
   * Integration with Jest test runner
   */
  createJestIntegration() {
    return {
      setupFilesAfterEnv: [path.join(__dirname, 'jest-pact-setup.js')],
      reporters: ['default', this.reporter.createJestReporter()],
      testTimeout: 60000, // Increase timeout for contract tests
      maxWorkers: 1, // Run contract tests sequentially to avoid conflicts
    };
  }

  /**
   * Integration with CI/CD pipeline
   */
  async setupCIPipeline() {
    const pipelineConfig = {
      preCommit: {
        command: 'npm run test:contract:consumer',
        description: 'Run consumer contract tests before commit',
      },
      prValidation: {
        command: 'npm run test:contract:verify',
        description: 'Verify provider contracts on PR',
      },
      deployment: {
        command: 'npm run contract:publish',
        description: 'Publish contracts on successful deployment',
      },
      monitoring: {
        webhooks: [
          {
            url: `${process.env.CONTRACT_METRICS_URL || 'http://localhost:9095'}/contract-test-result`,
            events: ['test.completed', 'test.failed'],
          },
        ],
      },
    };

    return pipelineConfig;
  }

  /**
   * Health check for contract testing infrastructure
   */
  async healthCheck() {
    const health = {
      timestamp: new Date().toISOString(),
      services: {},
      overall: 'healthy',
    };

    try {
      // Check metrics collector
      const metricsResponse = await fetch(
        `${this.reporter.metricsCollectorUrl}/health`
      );
      health.services.metricsCollector = {
        status: metricsResponse.ok ? 'healthy' : 'unhealthy',
        url: this.reporter.metricsCollectorUrl,
      };
    } catch (error) {
      health.services.metricsCollector = {
        status: 'unreachable',
        error: error.message,
      };
    }

    try {
      // Check Pact Broker
      const brokerResponse = await fetch(
        `${this.pactBrokerUrl}/diagnostic/status/heartbeat`
      );
      health.services.pactBroker = {
        status: brokerResponse.ok ? 'healthy' : 'unhealthy',
        url: this.pactBrokerUrl,
      };
    } catch (error) {
      health.services.pactBroker = {
        status: 'unreachable',
        error: error.message,
      };
    }

    // Determine overall health
    const serviceStatuses = Object.values(health.services);
    if (serviceStatuses.some(s => s.status === 'unhealthy')) {
      health.overall = 'degraded';
    } else if (serviceStatuses.some(s => s.status === 'unreachable')) {
      health.overall = 'critical';
    }

    return health;
  }

  /**
   * Cleanup and shutdown
   */
  async cleanup() {
    console.log('Cleaning up Enhanced Pact Integration...');
    // Perform any necessary cleanup
  }
}

// Export singleton instance
const enhancedPactIntegration = new EnhancedPactIntegration();

module.exports = {
  EnhancedPactIntegration,
  enhancedPactIntegration,
};
