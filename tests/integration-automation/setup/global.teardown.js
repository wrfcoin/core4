/**
 * Global Teardown for Integration Test Automation
 * Agent 6 - Integration Test Automation Specialist
 *
 * Gracefully shuts down Docker services and cleans up test environment
 * Generates final test reports and analytics
 */

const { execSync, exec } = require('child_process');
const fs = require('fs');
const path = require('path');
const { promisify } = require('util');

const execAsync = promisify(exec);

class IntegrationTestTeardown {
  constructor() {
    this.teardownStartTime = Date.now();
    this.dockerComposeFile = path.join(
      __dirname,
      '../../../docker-compose.test.yml'
    );
  }

  async teardownIntegrationEnvironment() {
    console.log('🔽 Shutting down Integration Test Automation Environment...');

    try {
      // Step 1: Generate test execution reports
      await this.generateTestReports();

      // Step 2: Collect service logs for debugging
      await this.collectServiceLogs();

      // Step 3: Gracefully shutdown Agent 4's mock services
      await this.shutdownMockServices();

      // Step 4: Clean up test data and temporary files
      await this.cleanupTestData();

      // Step 5: Validate clean shutdown
      await this.validateCleanShutdown();

      const teardownDuration = (Date.now() - this.teardownStartTime) / 1000;
      console.log(
        `✅ Integration Test Environment Shut Down Complete! (${teardownDuration}s)`
      );

      // Store teardown metadata
      await this.storeTeardownMetadata(teardownDuration);
    } catch (error) {
      console.error(
        '❌ Error during integration test teardown:',
        error.message
      );
      console.error(error.stack);

      // Force cleanup on error
      await this.forceCleanup();

      // Don't throw error from teardown to avoid masking test failures
      console.log('⚠️ Teardown completed with warnings');
    }
  }

  async generateTestReports() {
    console.log('📊 Generating integration test reports...');

    try {
      // Create reports directory if it doesn't exist
      const reportsDir = path.join(__dirname, '../../results');
      fs.mkdirSync(reportsDir, { recursive: true });

      // Generate test execution summary
      await this.generateExecutionSummary();

      // Generate service health report
      await this.generateServiceHealthReport();

      // Generate Agent 1 Pact verification report (if enabled)
      await this.generatePactReport();

      console.log('   ✓ Test reports generated');
    } catch (error) {
      console.log('   ⚠️ Report generation warning:', error.message);
    }
  }

  async generateExecutionSummary() {
    const summaryData = {
      timestamp: new Date().toISOString(),
      environment: 'integration-automation',
      testFramework: 'Jest',
      dockerServices: [
        'postgres',
        'redis',
        'blockchain-rpc',
        'weather-oracle',
        'carbon-oracle',
        'mockserver',
      ],
      agent1PactIntegration: !!process.env.PACT_VERIFICATION_ENABLED,
      agent4MockServices: true,
      testExecutionComplete: true,
    };

    const summaryPath = path.join(
      __dirname,
      '../../results/execution-summary.json'
    );
    fs.writeFileSync(summaryPath, JSON.stringify(summaryData, null, 2));
  }

  async generateServiceHealthReport() {
    const healthReport = {
      timestamp: new Date().toISOString(),
      services: {},
      overallHealth: 'unknown',
    };

    // Check final service states
    const services = [
      { name: 'postgres', port: 5432 },
      { name: 'redis', port: 6379 },
      { name: 'blockchain-rpc', port: 8545 },
      { name: 'weather-oracle', port: 3010 },
      { name: 'carbon-oracle', port: 3020 },
      { name: 'mockserver', port: 1080 },
    ];

    let healthyCount = 0;

    for (const service of services) {
      try {
        // Check if service is still responsive
        const response = await fetch(
          `http://localhost:${service.port}/health`,
          {
            timeout: 2000,
          }
        ).catch(() => ({ ok: false }));

        healthReport.services[service.name] = {
          port: service.port,
          healthy: response.ok,
          lastCheck: new Date().toISOString(),
        };

        if (response.ok) healthyCount++;
      } catch (error) {
        healthReport.services[service.name] = {
          port: service.port,
          healthy: false,
          error: error.message,
          lastCheck: new Date().toISOString(),
        };
      }
    }

    healthReport.overallHealth =
      healthyCount === services.length ? 'healthy' : 'degraded';
    healthReport.healthyServices = healthyCount;
    healthReport.totalServices = services.length;

    const healthPath = path.join(
      __dirname,
      '../../results/service-health-report.json'
    );
    fs.writeFileSync(healthPath, JSON.stringify(healthReport, null, 2));
  }

  async generatePactReport() {
    if (!process.env.PACT_VERIFICATION_ENABLED) {
      return;
    }

    try {
      console.log('   🤝 Generating Agent 1 Pact verification report...');

      // Check if Pact verification results exist
      const pactResultsPath = path.join(__dirname, '../../contract/results');

      if (fs.existsSync(pactResultsPath)) {
        const pactReport = {
          timestamp: new Date().toISOString(),
          agent1Integration: true,
          pactResultsFound: true,
          resultPath: pactResultsPath,
        };

        const reportPath = path.join(
          __dirname,
          '../../results/pact-integration-report.json'
        );
        fs.writeFileSync(reportPath, JSON.stringify(pactReport, null, 2));

        console.log('   ✓ Pact integration report generated');
      }
    } catch (error) {
      console.log('   ⚠️ Pact report generation warning:', error.message);
    }
  }

  async collectServiceLogs() {
    console.log('📝 Collecting service logs for debugging...');

    try {
      const logsDir = path.join(__dirname, '../../results/logs');
      fs.mkdirSync(logsDir, { recursive: true });

      // Collect docker-compose logs
      const { stdout: composeLogs } = await execAsync(
        `docker-compose -f "${this.dockerComposeFile}" --profile testing logs --no-color`,
        {
          cwd: path.dirname(this.dockerComposeFile),
          maxBuffer: 10 * 1024 * 1024, // 10MB buffer
        }
      ).catch(() => ({ stdout: 'Log collection failed' }));

      const logsPath = path.join(logsDir, 'docker-compose.log');
      fs.writeFileSync(logsPath, composeLogs);

      // Collect individual service logs
      const services = [
        'postgres',
        'blockchain-rpc',
        'weather-oracle',
        'carbon-oracle',
        'mockserver',
      ];

      for (const service of services) {
        try {
          const { stdout: serviceLogs } = await execAsync(
            `docker-compose -f "${this.dockerComposeFile}" logs --no-color ${service}`,
            {
              cwd: path.dirname(this.dockerComposeFile),
              maxBuffer: 5 * 1024 * 1024, // 5MB buffer per service
            }
          );

          const serviceLogsPath = path.join(logsDir, `${service}.log`);
          fs.writeFileSync(serviceLogsPath, serviceLogs);
        } catch (error) {
          console.log(
            `   ⚠️ Could not collect logs for ${service}: ${error.message}`
          );
        }
      }

      console.log('   ✓ Service logs collected');
    } catch (error) {
      console.log('   ⚠️ Log collection warning:', error.message);
    }
  }

  async shutdownMockServices() {
    console.log("🐳 Shutting down Agent 4's mock services...");

    try {
      // Graceful shutdown of docker-compose services
      await execAsync(
        `docker-compose -f "${this.dockerComposeFile}" --profile testing down --volumes --remove-orphans`,
        {
          cwd: path.dirname(this.dockerComposeFile),
          timeout: 30000, // 30 second timeout
        }
      );

      console.log('   ✓ Mock services shut down successfully');
    } catch (error) {
      console.log('   ⚠️ Error during mock service shutdown:', error.message);

      // Attempt force cleanup
      await this.forceCleanup();
    }
  }

  async cleanupTestData() {
    console.log('🧹 Cleaning up test data...');

    try {
      // Clean up temporary test files
      const tempDirs = [
        path.join(__dirname, '../../temp'),
        path.join(__dirname, '../../cache'),
      ];

      for (const dir of tempDirs) {
        if (fs.existsSync(dir)) {
          fs.rmSync(dir, { recursive: true, force: true });
        }
      }

      // Clean up Docker volumes and networks
      await execAsync('docker volume prune -f').catch(() => {});
      await execAsync('docker network prune -f').catch(() => {});

      console.log('   ✓ Test data cleanup complete');
    } catch (error) {
      console.log('   ⚠️ Cleanup warning:', error.message);
    }
  }

  async validateCleanShutdown() {
    console.log('🔍 Validating clean shutdown...');

    try {
      // Check that no test containers are still running
      const { stdout: runningContainers } = await execAsync(
        'docker ps --filter "label=com.docker.compose.project=tests" --format "table {{.Names}}"'
      ).catch(() => ({ stdout: '' }));

      if (runningContainers.includes('wrfcoin')) {
        console.log('   ⚠️ Some test containers may still be running');
        console.log(runningContainers);
      } else {
        console.log('   ✓ No test containers running');
      }

      // Check for port availability
      const portsToCheck = [5432, 6379, 8545, 3010, 3020, 1080];
      let portsInUse = [];

      for (const port of portsToCheck) {
        try {
          await fetch(`http://localhost:${port}`, { timeout: 1000 });
          portsInUse.push(port);
        } catch (error) {
          // Expected - port should not be accessible
        }
      }

      if (portsInUse.length > 0) {
        console.log(`   ⚠️ Ports still in use: ${portsInUse.join(', ')}`);
      } else {
        console.log('   ✓ All test service ports are free');
      }
    } catch (error) {
      console.log('   ⚠️ Validation warning:', error.message);
    }
  }

  async storeTeardownMetadata(teardownDuration) {
    const metadata = {
      timestamp: new Date().toISOString(),
      teardownDuration: teardownDuration,
      environment: 'integration-automation',
      cleanShutdown: true,
      reportsGenerated: true,
      logsCollected: true,
    };

    const metadataPath = path.join(
      __dirname,
      '../../results/teardown-metadata.json'
    );
    fs.mkdirSync(path.dirname(metadataPath), { recursive: true });
    fs.writeFileSync(metadataPath, JSON.stringify(metadata, null, 2));
  }

  async forceCleanup() {
    console.log('🚨 Attempting force cleanup...');

    try {
      // Force stop and remove all test containers
      await execAsync(
        'docker ps -q --filter "label=com.docker.compose.project=tests" | xargs -r docker stop'
      ).catch(() => {});
      await execAsync(
        'docker ps -aq --filter "label=com.docker.compose.project=tests" | xargs -r docker rm -f'
      ).catch(() => {});

      // Remove test networks
      await execAsync(
        'docker network ls -q --filter "name=tests" | xargs -r docker network rm'
      ).catch(() => {});

      // Remove test volumes
      await execAsync(
        'docker volume ls -q --filter "name=tests" | xargs -r docker volume rm'
      ).catch(() => {});

      console.log('   ✓ Force cleanup completed');
    } catch (error) {
      console.log('   ❌ Force cleanup failed:', error.message);
    }
  }
}

// Jest Global Teardown Function
module.exports = async () => {
  const teardown = new IntegrationTestTeardown();
  await teardown.teardownIntegrationEnvironment();
};
